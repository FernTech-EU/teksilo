// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `PopoverSurface` — the themed panel a popover's content sits in.
//!
//! Style infrastructure, not a widget an app mounts: `RecipePopoverStyle` (and
//! any `PopoverStyle` replacing it) constructs one in `make_body`, and
//! `PopoverWidget` shows the result as its overlay. It lived in `popover.rs`
//! beside the standalone `Popover` widget until that type was removed; the two
//! were never related beyond sharing a file.

use teksilo_canvas::{Canvas, EdgeInsets, Path, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::overlay::OverlayPlacement;
use teksilo_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};

pub struct PopoverSurface {
    content_id: Option<WidgetId>,
    pending_content: Option<PendingChild>,
    placement: OverlayPlacement,
    show_caret: bool,
    caret_size: f32,
    /// Accessible name for the dialog node — propagated from the trigger label.
    name: String,
    /// Inset between the panel edge and the wrapped content. Defaulted
    /// per `PopoverVariant` by `RecipePopoverStyle` (16 px for
    /// Default/Tooltip, zero for Menu so menu rows reach the edge).
    content_padding: EdgeInsets,
    /// Surface fill role for the panel background + caret.
    background: SurfaceRole,
    /// Panel corner radius in logical pixels.
    corner_radius: f32,
    /// When true the surface emits no semantic node (`set_hidden`) —
    /// used by the Menu variant, where the caller (`MenuList`,
    /// `DropdownPanel`, `SuggestionListBox`) already carries the
    /// container role. Default/Tooltip surfaces emit `Role::Dialog`.
    presentational: bool,
}

impl PopoverSurface {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        content: PendingChild,
        placement: OverlayPlacement,
        show_caret: bool,
        caret_size: f32,
        name: String,
        content_padding: EdgeInsets,
        background: SurfaceRole,
        corner_radius: f32,
        presentational: bool,
    ) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            placement,
            show_caret,
            caret_size,
            name,
            content_padding,
            background,
            corner_radius,
            presentational,
        }
    }

    /// Which side of the panel rect is attached to the trigger and
    /// should suppress shadow drawing. Derived from `placement` plus
    /// the active layout direction (resolved at paint time):
    /// - `Below*` / `NearAnchor` → anchor sits above ⇒ Top.
    /// - `Above` → anchor sits below ⇒ Bottom.
    /// - `TrailingEdge` → anchor sits on the leading side ⇒ Left in
    ///   LTR, Right in RTL.
    /// - Anything else (Centered, AtPointer, BottomCenter) → not
    ///   visually attached ⇒ no suppression.
    fn attached_shadow_side(
        &self,
        layout_direction: teksilo_core::environment::LayoutDirection,
    ) -> Option<crate::shadow::AttachedSide> {
        use teksilo_core::environment::LayoutDirection;
        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => Some(crate::shadow::AttachedSide::Top),
            OverlayPlacement::Above => Some(crate::shadow::AttachedSide::Bottom),
            OverlayPlacement::TrailingEdge => match layout_direction {
                LayoutDirection::LeftToRight => Some(crate::shadow::AttachedSide::Left),
                LayoutDirection::RightToLeft => Some(crate::shadow::AttachedSide::Right),
            },
            _ => None,
        }
    }

    fn caret_insets(&self) -> (f32, f32) {
        if !self.show_caret {
            return (0.0, 0.0);
        }

        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => (self.caret_size, 0.0),
            OverlayPlacement::Above => (0.0, self.caret_size),
            _ => (0.0, 0.0),
        }
    }

    fn panel_bounds(&self, bounds: Rect) -> Rect {
        let (top, bottom) = self.caret_insets();
        Rect::new(
            bounds.x,
            bounds.y + top,
            bounds.width,
            (bounds.height - top - bottom).max(0.0),
        )
    }

    fn caret_path(&self, bounds: Rect) -> Option<Path> {
        if !self.show_caret {
            return None;
        }

        let panel = self.panel_bounds(bounds);
        let center_x = panel.x + panel.width.min(56.0) / 2.0 + 18.0;
        let half = self.caret_size;
        let mut path = Path::new();

        match self.placement {
            OverlayPlacement::Below
            | OverlayPlacement::BelowPreferred
            | OverlayPlacement::NearAnchor { .. } => {
                path.move_to(Point::new(center_x - half, panel.y));
                path.line_to(Point::new(center_x, bounds.y));
                path.line_to(Point::new(center_x + half, panel.y));
                path.close();
                Some(path)
            }
            OverlayPlacement::Above => {
                let bottom = panel.bottom();
                path.move_to(Point::new(center_x - half, bottom));
                path.line_to(Point::new(center_x, bottom + self.caret_size));
                path.line_to(Point::new(center_x + half, bottom));
                path.close();
                Some(path)
            }
            _ => None,
        }
    }
}

impl std::fmt::Debug for PopoverSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverSurface").finish()
    }
}

impl Widget for PopoverSurface {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let inset_w = self.content_padding.leading + self.content_padding.trailing;
        let inset_h = self.content_padding.top + self.content_padding.bottom;
        let (caret_top, caret_bottom) = self.caret_insets();
        self.content_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset_w).max(0.0)),
                        height: proposal
                            .height
                            .map(|height| (height - inset_h - caret_top - caret_bottom).max(0.0)),
                    },
                )
            })
            .map(|size| {
                Size::new(
                    size.width + inset_w,
                    size.height + inset_h + caret_top + caret_bottom,
                )
            })
            .unwrap_or_else(|| proposal.resolve(200.0, 80.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let panel = self.panel_bounds(bounds);
        let pad = self.content_padding;
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(panel.x + pad.leading, panel.y + pad.top);
            child.size = Size::new(
                (panel.width - pad.leading - pad.trailing).max(0.0),
                (panel.height - pad.top - pad.bottom).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let panel = self.panel_bounds(bounds);
        let radius = CornerRadius::uniform(self.corner_radius);
        let fill = self.background.resolve(&ctx.theme.colors);
        crate::shadow::paint_layered_shadow(
            canvas,
            panel,
            radius,
            &ctx.theme.shape.shadow_sm,
            &ctx.theme.shape.shadow_inner_sm,
            crate::styles::recipe_popover_style::POPOVER_SHADOW_DENSITY,
            self.attached_shadow_side(ctx.layout_direction),
        );
        // The caret extends into the just-suppressed zone (between
        // panel and trigger). It's painted unshaded below — that's
        // intentional, the caret reads as part of the trigger-attach
        // region, not as a separate elevated surface.
        canvas.fill_rounded_rect(panel, radius, fill);
        canvas.stroke_rounded_rect(
            panel,
            radius,
            ctx.theme.colors.border,
            ctx.theme.shape.border_width,
        );
        if let Some(path) = self.caret_path(bounds) {
            canvas.fill_path(&path, fill);
            canvas.stroke_path(&path, ctx.theme.colors.border, ctx.theme.shape.border_width);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.presentational {
            // Menu-variant container: the caller (`MenuList`,
            // `DropdownPanel`, `SuggestionListBox`) already owns the
            // semantic role, so the surface contributes nothing.
            builder.set_hidden();
            return;
        }
        // Popover surface is modeled as a non-modal Dialog: ARIA has
        // no dedicated popover role, and Role::Dialog without
        // `set_modal` is the standard fallback for panels that float
        // over other content without blocking it. Every dialog node
        // must have an accessible name; use the trigger's label.
        builder.set_role(teksilo_core::accesskit::Role::Dialog);
        builder.set_name(&self.name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}
