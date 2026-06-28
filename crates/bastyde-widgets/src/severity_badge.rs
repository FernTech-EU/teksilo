// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared severity badge — the two-tone status glyph used by every
//! status surface in the catalog: [`Banner`](crate::banner::Banner),
//! [`Toast`](crate::toast::Toast),
//! [`MessageBox`](crate::message_box::MessageBox), and the
//! [`NotificationLog`](crate::notification::log::NotificationLog).
//!
//! Each badge is a filled status-colored disc (or a triangle for
//! `Warning`, matching the universal warning-sign convention) carrying
//! a crisp symbol on top: `i` / `✓` / `!` / `✕` / `?`. It reads by
//! **shape AND color**, so the severities stay distinguishable for
//! colour-blind users — unlike a bare coloured dot.
//!
//! The artwork lives in `resources/icons/severity-*.svg` and is
//! composited as two tinted [`IconWidget`] layers so the disc and
//! symbol can carry independent, theme-reactive
//! tints. The symbol tint is chosen per-disc by relative luminance
//! (black on the light fills — amber, teal — white on the dark ones —
//! green, red) so the mark always meets contrast in both light and
//! dark themes. Both tints re-resolve on a theme change without a
//! rebuild (the icon colour binding is `RepaintOnly`).

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::BannerSeverity;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

use crate::primitives::IconWidget;
use crate::primitives::icon_widget::IconMode;

/// Which severity badge to render. A superset of [`BannerSeverity`]
/// that adds `Question` for `MessageBox` confirmation dialogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeverityIconKind {
    Info,
    Success,
    Warning,
    Error,
    Question,
}

impl SeverityIconKind {
    /// The filled backing shape — a triangle for `Warning`, a disc for
    /// everything else.
    fn disc_svg(self) -> &'static str {
        match self {
            Self::Warning => include_str!("../resources/icons/severity-triangle.svg"),
            _ => include_str!("../resources/icons/severity-disc.svg"),
        }
    }

    /// The symbol drawn on top of the backing shape.
    fn symbol_svg(self) -> &'static str {
        match self {
            Self::Info => include_str!("../resources/icons/severity-info.svg"),
            Self::Success => include_str!("../resources/icons/severity-success.svg"),
            Self::Warning => include_str!("../resources/icons/severity-warning.svg"),
            Self::Error => include_str!("../resources/icons/severity-error.svg"),
            Self::Question => include_str!("../resources/icons/severity-question.svg"),
        }
    }

    /// Tint for the backing shape, resolved against the active theme.
    /// Matches the status foreground tokens (`Question` reuses the
    /// accent, mirroring `MessageBox`'s historical mapping).
    fn disc_color(self, theme: &bastyde_core::Theme) -> Color {
        match self {
            Self::Info => theme.colors.status_info_fg,
            Self::Success => theme.colors.status_success_fg,
            Self::Warning => theme.colors.status_warning_fg,
            Self::Error => theme.colors.status_error_fg,
            Self::Question => theme.colors.accent,
        }
    }
}

impl From<BannerSeverity> for SeverityIconKind {
    fn from(severity: BannerSeverity) -> Self {
        match severity {
            BannerSeverity::Info => Self::Info,
            BannerSeverity::Success => Self::Success,
            BannerSeverity::Warning => Self::Warning,
            BannerSeverity::Error => Self::Error,
        }
    }
}

/// Pick a symbol tint that contrasts with the badge fill. Same rule
/// the theme uses for `text_on_accent`: black on a light fill, white
/// on a dark one. Keeps the `!` legible on amber and the `✓` legible
/// on green without per-severity tables.
fn symbol_color_for(disc: Color) -> Color {
    if disc.relative_luminance() > 0.4 {
        Color::from_hex("#000000")
    } else {
        Color::WHITE
    }
}

/// A two-tone severity badge. Construct with [`SeverityBadge::new`] and
/// drop it anywhere a leading status glyph is wanted — it is a plain
/// composing widget (no `BuildContext` needed at construction), so it
/// works equally from a `build()` body or a delegate that returns a
/// `Box<dyn Widget>`.
#[derive(Debug)]
pub(crate) struct SeverityBadge {
    kind: SeverityIconKind,
    size: f32,
    root_child_id: Option<WidgetId>,
}

impl SeverityBadge {
    pub(crate) fn new(kind: SeverityIconKind, size: f32) -> Self {
        Self {
            kind,
            size,
            root_child_id: None,
        }
    }
}

impl Widget for SeverityBadge {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let kind = self.kind;
        let theme_sig = ctx.theme_signal();
        let disc_color = theme_sig.map(move |t| kind.disc_color(t));
        let symbol_color = theme_sig.map(move |t| symbol_color_for(kind.disc_color(t)));

        let disc = ctx.add(
            IconWidget::from_svg(kind.disc_svg())
                .icon_size(self.size)
                .follow_text_scale(true)
                .mode(IconMode::Tintable)
                .color(disc_color),
        );
        let symbol = ctx.add(
            IconWidget::from_svg(kind.symbol_svg())
                .icon_size(self.size)
                .follow_text_scale(true)
                .mode(IconMode::Tintable)
                .color(symbol_color),
        );
        let root = ctx.add(
            crate::primitives::ZStack::new()
                .add_child(disc)
                .add_child(symbol),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(self.size, self.size))
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

    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        // The badge is decorative — the enclosing status surface
        // already carries the role + announcement.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::svg::SvgIcon;
    use bastyde_core::widget_tree::WidgetTree;

    const ALL: [SeverityIconKind; 5] = [
        SeverityIconKind::Info,
        SeverityIconKind::Success,
        SeverityIconKind::Warning,
        SeverityIconKind::Error,
        SeverityIconKind::Question,
    ];

    /// Every backing-shape SVG must parse and carry a filled shape — a
    /// malformed SVG silently falls back to empty (no badge), so guard
    /// against that here rather than discovering it on screen.
    #[test]
    fn disc_svgs_parse_with_a_fill() {
        for kind in ALL {
            let icon = SvgIcon::parse(kind.disc_svg())
                .unwrap_or_else(|e| panic!("{kind:?} disc svg failed to parse: {e:?}"));
            assert!(
                !icon.raw_path().is_empty(),
                "{kind:?} disc must carry a filled shape"
            );
        }
    }

    /// Every symbol SVG must parse and carry geometry (stroked mark
    /// and/or filled dot).
    #[test]
    fn symbol_svgs_parse_with_geometry() {
        for kind in ALL {
            let icon = SvgIcon::parse(kind.symbol_svg())
                .unwrap_or_else(|e| panic!("{kind:?} symbol svg failed to parse: {e:?}"));
            assert!(
                !icon.is_empty(),
                "{kind:?} symbol must carry a visible mark"
            );
        }
    }

    /// Warning uses the triangle backing; the rest use the disc.
    #[test]
    fn warning_uses_triangle_others_use_disc() {
        let disc = SeverityIconKind::Info.disc_svg();
        assert_eq!(SeverityIconKind::Success.disc_svg(), disc);
        assert_eq!(SeverityIconKind::Error.disc_svg(), disc);
        assert_eq!(SeverityIconKind::Question.disc_svg(), disc);
        assert_ne!(SeverityIconKind::Warning.disc_svg(), disc);
    }

    /// The symbol tint flips to black on the light fills (so the mark
    /// stays legible) and white on the dark ones.
    #[test]
    fn symbol_color_contrasts_with_fill() {
        let theme = bastyde_core::presets::intui::light();
        // Amber warning + teal info are light → black symbol.
        assert_eq!(
            symbol_color_for(SeverityIconKind::Warning.disc_color(&theme)),
            Color::from_hex("#000000")
        );
        // Green success + red error are dark → white symbol.
        assert_eq!(
            symbol_color_for(SeverityIconKind::Success.disc_color(&theme)),
            Color::WHITE
        );
        assert_eq!(
            symbol_color_for(SeverityIconKind::Error.disc_color(&theme)),
            Color::WHITE
        );
    }

    /// The composite badge builds and lays out to the requested size.
    #[test]
    fn badge_builds_and_lays_out_to_size() {
        for kind in ALL {
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            let id = tree.add(SeverityBadge::new(kind, 24.0));
            tree.layout(SizeProposal::exact(24.0, 24.0));
            let b = tree.bounds(id);
            assert!(
                (b.width - 24.0).abs() < 0.5 && (b.height - 24.0).abs() < 0.5,
                "{kind:?} badge should be 24×24, got {}×{}",
                b.width,
                b.height
            );
        }
    }
}
