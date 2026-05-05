//! Banner — persistent inline status strip (info / success / warning / error).
//!
//! A non-transient, full-width callout for app-level conditions: deprecation
//! notices, "you have unsaved changes", trial-expiry warnings, license
//! issues, restored-from-cache notices, etc. Distinct from
//! [`Snackbar`](crate::snackbar::Snackbar) (transient, corner-anchored) and
//! [`MessageBox`](crate::message_box::MessageBox) (modal).
//!
//! ```ignore
//! Banner::warning(tr!("unsaved_changes"))
//!     .description(tr!("close_loses_changes"))
//!     .action(Button::new(tr!("save_now"))
//!         .on_activate_fn(|ctx| ctx.send_intent(AppIntent::SaveNow)))
//!     .on_dismiss(|ctx| ctx.send_intent(AppIntent::DismissBanner))
//! ```

use fern_canvas::{Canvas, Path, Point, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole, TextRole, TextStyleRole, VAlignment};

use crate::built_in_button::BuiltInButton;
use crate::primitives::{Expand, HStack, Padding, RectWidget, TextWidget, VStack, ZStack};

/// Banner severity level. Drives the surface tint, glyph color, and
/// glyph shape. Apps with a "neutral" callout requirement should use
/// a [`Card`](crate::card::Card) instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerSeverity {
    /// Informational notice — accent-tinted background, circle glyph.
    Info,
    /// Success / confirmation — green-tinted background, circle glyph.
    Success,
    /// Non-fatal warning — amber-tinted background, triangle glyph.
    Warning,
    /// Error / critical condition — red-tinted background, circle glyph.
    Error,
}

impl BannerSeverity {
    fn surface(self) -> SurfaceRole {
        match self {
            Self::Info => SurfaceRole::StatusInfo,
            Self::Success => SurfaceRole::StatusSuccess,
            Self::Warning => SurfaceRole::StatusWarning,
            Self::Error => SurfaceRole::StatusError,
        }
    }

    fn glyph_color(self, theme: &fern_tokens::Theme) -> fern_tokens::Color {
        match self {
            Self::Info => theme.colors.status_info_fg,
            Self::Success => theme.colors.status_success_fg,
            Self::Warning => theme.colors.status_warning_fg,
            Self::Error => theme.colors.status_error_fg,
        }
    }
}

/// Small leaf widget that paints the severity glyph (circle or
/// triangle). Sized via `BannerStyle::glyph_size` at paint time.
struct SeverityGlyph {
    severity: BannerSeverity,
    size: f32,
}

impl std::fmt::Debug for SeverityGlyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeverityGlyph")
            .field("severity", &self.severity)
            .finish()
    }
}

impl Widget for SeverityGlyph {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        proposal.resolve(self.size, self.size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.severity.glyph_color(ctx.theme);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let half = (bounds.width.min(bounds.height) / 2.0).max(2.0);
        let path = match self.severity {
            BannerSeverity::Warning => {
                let mut p = Path::new();
                p.move_to(Point::new(cx, cy - half));
                p.line_to(Point::new(cx + half, cy + half));
                p.line_to(Point::new(cx - half, cy + half));
                p.close();
                p
            }
            _ => Path::circle(Point::new(cx, cy), half),
        };
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

/// A persistent inline status strip.
pub struct Banner {
    severity: BannerSeverity,
    title: String,
    description: Option<String>,
    action: Option<Box<dyn Widget>>,
    on_dismiss: Option<Box<dyn Fn(&mut EventContext)>>,
    root_child_id: Option<WidgetId>,
}

impl Banner {
    fn new(severity: BannerSeverity, title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        Self {
            severity,
            title: ls.resolve_now(),
            description: None,
            action: None,
            on_dismiss: None,
            root_child_id: None,
        }
    }

    /// Construct an info-severity banner.
    pub fn info(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        Self::new(BannerSeverity::Info, title)
    }

    /// Construct a success-severity banner.
    pub fn success(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        Self::new(BannerSeverity::Success, title)
    }

    /// Construct a warning-severity banner.
    pub fn warning(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        Self::new(BannerSeverity::Warning, title)
    }

    /// Construct an error-severity banner.
    pub fn error(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        Self::new(BannerSeverity::Error, title)
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in
    /// `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn info_literal(title: impl Into<String>) -> Self {
        Self::info(fern_i18n::LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn success_literal(title: impl Into<String>) -> Self {
        Self::success(fern_i18n::LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn warning_literal(title: impl Into<String>) -> Self {
        Self::warning(fern_i18n::LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn error_literal(title: impl Into<String>) -> Self {
        Self::error(fern_i18n::LocalizedString::literal(title))
    }

    /// Optional secondary line of text rendered below the title.
    pub fn description(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.description = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `description(...)`.
    #[doc(hidden)]
    pub fn description_literal(self, text: impl Into<String>) -> Self {
        self.description(fern_i18n::LocalizedString::literal(text))
    }

    /// Trailing widget — typically a [`Button`](crate::button::Button) or
    /// an `HStack` of buttons. Placed before the optional dismiss button.
    pub fn action(mut self, widget: impl Widget + 'static) -> Self {
        self.action = Some(Box::new(widget));
        self
    }

    /// Attach a trailing dismiss (X) button. The closure runs when the
    /// user clicks it; the host is expected to remove the banner from the
    /// tree (typically by toggling a `Signal<bool>` driving a `Switcher`).
    pub fn on_dismiss(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(f));
        self
    }
}

impl std::fmt::Debug for Banner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Banner")
            .field("severity", &self.severity)
            .field("title", &self.title)
            .field("description", &self.description)
            .finish()
    }
}

impl Widget for Banner {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let severity = self.severity;
        let style = ctx.theme().components.banner;
        let radius = CornerRadius::uniform(style.corner_radius);

        // Background panel — status surface tint, no border (the surface
        // tokens already encode contrast with the page background).
        let bg = ctx.add(
            RectWidget::new()
                .background(ColorProp::SurfaceRole(severity.surface()))
                .corner_radius(radius),
        );

        // Severity glyph.
        let glyph = ctx.add(SeverityGlyph {
            severity,
            size: style.glyph_size,
        });

        // Title + optional description column.
        let title = ctx.add(
            TextWidget::new_literal(&self.title)
                .style(TextStyleRole::BodyBold)
                .bind_color(TextRole::Primary)
                .single_line(),
        );
        let mut text_column = VStack::new()
            .spacing(style.title_description_gap)
            .add_child(title);
        if let Some(description) = &self.description {
            let desc = ctx.add(
                TextWidget::new_literal(description)
                    .style(TextStyleRole::Body)
                    .bind_color(TextRole::Secondary),
            );
            text_column = text_column.add_child(desc);
        }
        let text_column_id = ctx.add(text_column);

        // Row layout: [glyph] [text + spacer expanding] [action] [dismiss]
        let mut row = HStack::new()
            .spacing(style.content_gap)
            .alignment(VAlignment::Center)
            .add_child(glyph)
            .add_child(ctx.add(Expand::horizontal().child_id(text_column_id)));
        if let Some(action) = self.action.take() {
            row = row.add_child(ctx.add_boxed(action));
        }
        if let Some(on_dismiss) = self.on_dismiss.take() {
            // BuiltInButton::clear() ships with its own translated
            // "Clear" tooltip / a11y label — adequate for a banner
            // dismiss button without inventing a new i18n key.
            let btn = BuiltInButton::clear().on_activate_fn(move |c| on_dismiss(c));
            row = row.add_child(ctx.add(btn));
        }

        let row_id = ctx.add(row);
        let padded = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal).child_id(row_id),
        );

        let root = ctx.add(ZStack::new().add_child(bg).add_child(padded));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // Banners are a "fill width, hug height" surface. ZStack queries
        // its children with `SizeProposal::unspecified`, so delegating
        // straight to `child_size` would return the row's natural width
        // and the banner would collapse to its content. Take the
        // proposed width as the source of truth and use the inner
        // height (computed against that width) for the visual.
        let inner_proposal = SizeProposal {
            width: proposal.width,
            height: None,
        };
        let inner = self
            .root_child_id
            .and_then(|id| ctx.child_size(id, inner_proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
        let width = proposal.width.unwrap_or(inner.width);
        fern_canvas::Size::new(width, inner.height).into()
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
        // Banners convey status without consuming focus. `Role::Status`
        // matches the ARIA pattern for a non-modal status region; pair
        // with `Live::Polite` so screen readers announce changes.
        builder.set_role(fern_core::accesskit::Role::Status);
        builder.set_live(fern_core::accesskit::Live::Polite);
        // Use the title alone as the AT name; the description is read by
        // descending into the body text widget.
        builder.set_name(self.title.clone());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn banner_builds_and_lays_out() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Banner::warning_literal("Unsaved changes")
                .description_literal("Close will discard your edits."),
        );
        tree.layout(SizeProposal {
            width: Some(640.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 640.0).abs() < 0.01);
        assert!(b.height > 0.0);
    }

    #[test]
    fn banner_a11y_role_and_name() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Banner::info_literal("Heads up"));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::Status);
        assert_eq!(info.name(), Some("Heads up"));
    }

    #[test]
    fn banner_fills_width_inside_vstack() {
        // Regression: ZStack queries children with `unspecified` proposal,
        // so a naive `child_size(root, proposal)` delegate makes Banner
        // collapse to its content width inside a normal VStack parent.
        // Banner's `layout_response` overrides the width to the proposal.
        use crate::primitives::VStack;
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let banner = Banner::warning_literal("Unsaved changes")
            .description_literal("Close will discard your edits.");
        let stack_id = tree.add(VStack::new().spacing(8.0).child(banner));
        tree.layout(SizeProposal {
            width: Some(640.0),
            height: None,
        });

        // Walk the VStack's first descendant Banner (Role::Status) and
        // check its bounds.
        let mut queue = vec![stack_id];
        let mut banner_bounds = None;
        while let Some(id) = queue.pop() {
            let info = tree.accessibility_node(id);
            if info.role() == fern_core::accesskit::Role::Status {
                banner_bounds = Some(tree.bounds(id));
                break;
            }
            queue.extend(tree.children(id));
        }
        let b = banner_bounds.expect("Banner should be in the tree under the VStack");
        assert!(
            (b.width - 640.0).abs() < 0.5,
            "Banner inside VStack should span the proposed width 640 dp, got {}",
            b.width
        );
    }

    #[test]
    fn banner_with_dismiss_emits_clear_button() {
        use std::cell::Cell;
        use std::rc::Rc;
        let dismissed = Rc::new(Cell::new(false));
        let dismissed_clone = dismissed.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Banner::error_literal("Disk almost full")
                .on_dismiss(move |_| dismissed_clone.set(true)),
        );
        tree.layout(SizeProposal {
            width: Some(640.0),
            height: None,
        });
        // Find the clear button by tooltip / a11y label and click it.
        let dismiss = tree
            .find_by_label("Clear")
            .or_else(|| tree.find_by_label("Effacer"))
            .expect("dismiss button should be present");
        tree.dispatch_event(fern_core::event::WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(dismiss),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(dismissed.get(), "on_dismiss should have fired");
    }
}
