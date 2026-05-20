//! Default `LinkStyle` impl driven by paint-recipe data.
//!
//! `RecipeLinkStyle` ships the IntUI link chrome: idle / hover /
//! visited text colours via the standard `TextRole::Link*` roles, a
//! 1 px underline matching the text colour, and a corner-rounded focus
//! ring that appears only on keyboard focus.

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{LinkStyle, LinkStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, TextRole, TextStyleRole};

use crate::primitives::{FixedSize, RectWidget, TextWidget, VStack, ZStack};

// IntUI design tokens for Link. The recipe owns its own dimensions.
pub const LINK_CORNER_RADIUS: f32 = 4.0;
pub const LINK_UNDERLINE_THICKNESS: f32 = 1.0;

/// Per-state text role for a link, given its four interaction signals
/// plus a static disabled hint. Exposed so custom `LinkStyle`
/// implementations can reuse the IntUI mapping when they only want to
/// swap the underline / focus-ring policy. Hover and pressed both map
/// to `LinkHover`; visited overrides idle but is itself overridden by
/// hover (standard web convention); disabled wins outright.
pub fn link_text_role(
    hovered: bool,
    pressed: bool,
    focused: bool,
    visited: bool,
    disabled: bool,
) -> TextRole {
    if disabled {
        return TextRole::Disabled;
    }
    if hovered || pressed {
        return TextRole::LinkHover;
    }
    if visited {
        return TextRole::LinkVisited;
    }
    let _ = focused; // focus is signalled by the border ring, not the text colour.
    TextRole::Link
}

/// Default `LinkStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeLinkStyle;

impl LinkStyle for RecipeLinkStyle {
    fn make_body(&self, cfg: &LinkStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Derived `Signal<TextRole>` combining the four state signals
        // plus the reactive disabled signal. The text widget and the
        // underline both bind to this role, so the underline tracks
        // the text colour through every state transition.
        //
        // Note: `is_disabled` is now reactive (a `Signal<bool>` sourced
        // from the arena's `effective_enabled` chain). At
        // wide-enough wrap widths, the leaves' `ColorProp::resolve`
        // would already substitute `TextRole::Disabled` when
        // `effective_enabled = false` — but the link style explicitly
        // returns a `Disabled` role here too so the underline path
        // and any test/snapshot consuming `LinkStyleConfig.is_disabled`
        // stay coherent.
        let text_role: Signal<TextRole> = cfg
            .is_hovered
            .zip3(&cfg.is_pressed, &cfg.is_focused)
            .zip(&cfg.is_visited)
            .zip(&cfg.is_disabled)
            .map(move |(((h, p, f), v), d)| link_text_role(*h, *p, *f, *v, *d));

        let text_id = ctx.add(
            TextWidget::new_literal(&cfg.text)
                .style(TextStyleRole::Body)
                .bind_color(text_role.clone())
                .single_line()
                .a11y_hidden(),
        );

        // 1 px underline below the text, bound to the same text-role
        // signal as the background colour so the line matches.
        let underline = ctx.add(RectWidget::new().bind_background(text_role));
        let underline_sized = ctx.add(
            FixedSize::new()
                .bind_height(LINK_UNDERLINE_THICKNESS)
                .child_id(underline),
        );

        let content_id = ctx.add(
            VStack::new()
                .spacing(0.0)
                .add_child(text_id)
                .add_child(underline_sized),
        );

        // Focus ring — accent border drawn only when the link holds
        // keyboard focus. IntUI convention paints the ring on the link
        // itself (not as a separate outline) so the focus envelope
        // matches the rounded text bounds.
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let focus_border_role = cfg.is_focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Transparent
            }
        });
        let focus_border_width = cfg
            .is_focused
            .map(move |f| if *f { focus_ring_width } else { 0.0 });
        let focus_rect_id = ctx.add(
            RectWidget::new()
                .bind_border_color(focus_border_role)
                .bind_border_width(focus_border_width)
                .corner_radius(CornerRadius::uniform(LINK_CORNER_RADIUS)),
        );

        ctx.add(ZStack::new().add_child(focus_rect_id).add_child(content_id))
    }
}
