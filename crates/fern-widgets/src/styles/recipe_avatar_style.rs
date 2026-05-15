//! Default `AvatarStyle` impl driven by paint-recipe data.
//!
//! `RecipeAvatarStyle` ports the IntUI avatar chrome exactly: the
//! shape-aware background fill (with hash-derived palette pick when no
//! caller override is supplied), the optional border ring drawn over
//! the inner content to mask any image bleed at the rim, the keyboard
//! focus ring hugging the configured shape, and the presence
//! indicator dot positioned at one of the four corners with a
//! `surface_main`-coloured outline.
//!
//! The chrome helpers (`hash_pick_palette_color`, `auto_contrast_text`,
//! `paint_border`, `paint_focus_ring`, `fnv1a_64`, `avatar_pixel_size`)
//! live here so custom `AvatarStyle` implementations can reuse them
//! when they only want to swap one piece of the chrome.

use fern_canvas::{Canvas, Paint, Point, Rect, Size, SizeProposal, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Signal;
use fern_core::styles::{
    AvatarCorner, AvatarPresence, AvatarShape, AvatarSize, AvatarStyle, AvatarStyleConfig,
};
use fern_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

// ─── IntUI design tokens for Avatar ────────────────────────────────
// Relocated from `theme.components.avatar` in Stage C of the group-5
// styling migration — the recipe owns its own dimensions. `Avatar` (and
// its `InitialsLeaf` sub-widget) reads the public `pub const`s below
// directly when it needs sizing data outside the chrome frame.

pub const AVATAR_SIZE_SMALL: f32 = 24.0;
pub const AVATAR_SIZE_MEDIUM: f32 = 32.0;
pub const AVATAR_SIZE_LARGE: f32 = 48.0;
pub const AVATAR_SIZE_X_LARGE: f32 = 64.0;

/// Default border (ring) thickness when `.border()` is called without
/// an explicit width override.
pub const AVATAR_BORDER_DEFAULT: f32 = 2.0;

/// Presence dot diameter as a fraction of avatar diameter.
pub const AVATAR_PRESENCE_DIAMETER_RATIO: f32 = 0.28;
pub const AVATAR_PRESENCE_DIAMETER_MIN: f32 = 8.0;
pub const AVATAR_PRESENCE_DIAMETER_MAX: f32 = 20.0;
/// Outline drawn around the presence dot.
pub const AVATAR_PRESENCE_OUTLINE_WIDTH: f32 = 1.5;
/// Inset of the presence dot from the avatar's bounding box edge.
pub const AVATAR_PRESENCE_INSET: f32 = 0.0;

/// Initials font-size as a fraction of avatar diameter.
pub const AVATAR_FONT_RATIO_1CHAR: f32 = 0.45;
pub const AVATAR_FONT_RATIO_2CHAR: f32 = 0.40;

/// Corner-radius ratio for `AvatarShape::RoundedSquare`.
pub const AVATAR_ROUNDED_RADIUS_RATIO: f32 = 0.25;

/// Resolve a discrete `AvatarSize` to a logical-pixel side length
/// using the recipe's size table. `Custom(px)` is clamped to at least
/// 1 px.
pub fn avatar_pixel_size(size: AvatarSize) -> f32 {
    match size {
        AvatarSize::Small => AVATAR_SIZE_SMALL,
        AvatarSize::Medium => AVATAR_SIZE_MEDIUM,
        AvatarSize::Large => AVATAR_SIZE_LARGE,
        AvatarSize::XLarge => AVATAR_SIZE_X_LARGE,
        AvatarSize::Custom(px) => px.max(1.0),
    }
}

/// FNV-1a 64-bit hash — deterministic palette-bucket selection.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Pick a colour from the theme's chart palette deterministically
/// from `seed`. Empty palette falls back to a neutral grey so the
/// widget still renders.
pub fn hash_pick_palette_color(seed: &str, theme: &fern_core::Theme) -> Color {
    let palette = &theme.colors.chart_palette;
    if palette.is_empty() {
        return Color::from_rgb(0.5, 0.5, 0.5);
    }
    let h = fnv1a_64(seed.as_bytes());
    let idx = (h as usize) % palette.len();
    palette[idx]
}

/// Auto-contrast foreground for a given background.
pub fn auto_contrast_text(bg: Color) -> Color {
    if bg.relative_luminance() < 0.5 {
        Color::WHITE
    } else {
        Color::from_rgb(0.121, 0.121, 0.121)
    }
}

/// Stroke a focus ring outside the avatar's content bounds. Hugs the
/// configured shape so a square avatar gets a square ring.
pub fn paint_focus_ring(
    canvas: &mut Canvas,
    bounds: Rect,
    shape: AvatarShape,
    rounded_radius_ratio: f32,
    offset: f32,
    width: f32,
    color: Color,
) {
    let outset = offset + width / 2.0;
    let outer = Rect::new(
        bounds.x - outset,
        bounds.y - outset,
        bounds.width + outset * 2.0,
        bounds.height + outset * 2.0,
    );
    match shape {
        AvatarShape::Circle => {
            let radius = outer.width.min(outer.height) / 2.0;
            let center =
                Point::new(outer.x + outer.width / 2.0, outer.y + outer.height / 2.0);
            canvas.stroke_circle(
                center,
                radius,
                Paint::from(color),
                StrokeStyle::solid(width),
            );
        }
        AvatarShape::RoundedSquare => {
            let r = bounds.width.min(bounds.height) * rounded_radius_ratio + outset;
            canvas.stroke_rounded_rect(
                outer,
                CornerRadius::uniform(r),
                color,
                StrokeStyle::solid(width),
            );
        }
        AvatarShape::Square => {
            canvas.stroke_rounded_rect(
                outer,
                CornerRadius::uniform(0.0),
                color,
                StrokeStyle::solid(width),
            );
        }
    }
}

/// Stroke a border ring inside the avatar's content bounds.
pub fn paint_border(
    canvas: &mut Canvas,
    bounds: Rect,
    shape: AvatarShape,
    rounded_radius_ratio: f32,
    width: f32,
    color: Color,
) {
    let half = width / 2.0;
    let inner = Rect::new(
        bounds.x + half,
        bounds.y + half,
        (bounds.width - width).max(0.0),
        (bounds.height - width).max(0.0),
    );
    match shape {
        AvatarShape::Circle => {
            let radius = inner.width.min(inner.height) / 2.0;
            let center =
                Point::new(inner.x + inner.width / 2.0, inner.y + inner.height / 2.0);
            canvas.stroke_circle(
                center,
                radius,
                Paint::from(color),
                StrokeStyle::solid(width),
            );
        }
        AvatarShape::RoundedSquare => {
            let r = inner.width.min(inner.height) * rounded_radius_ratio;
            canvas.stroke_rounded_rect(
                inner,
                CornerRadius::uniform(r),
                color,
                StrokeStyle::solid(width),
            );
        }
        AvatarShape::Square => {
            canvas.stroke_rounded_rect(
                inner,
                CornerRadius::uniform(0.0),
                color,
                StrokeStyle::solid(width),
            );
        }
    }
}

/// Default `AvatarStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeAvatarStyle;

impl AvatarStyle for RecipeAvatarStyle {
    fn make_body(&self, cfg: &AvatarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(AvatarChromeFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            shape: cfg.shape,
            presence: cfg.presence.clone(),
            presence_corner: cfg.presence_corner,
            is_focused: cfg.is_focused.clone(),
            background: cfg.background_override.clone(),
            border_color: cfg.border_color_override.clone(),
            border_width: cfg.border_width_override,
            seed: cfg.seed.clone(),
        })
    }
}

/// Internal container that paints the avatar chrome (shape-aware
/// background fill, border ring, keyboard focus ring, presence dot)
/// around the pre-built content child. Mirrors the pre-migration
/// `Avatar::paint` exactly.
struct AvatarChromeFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    shape: AvatarShape,
    presence: Option<AvatarPresence>,
    presence_corner: AvatarCorner,
    is_focused: Signal<bool>,
    background: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Option<f32>,
    seed: String,
}

impl std::fmt::Debug for AvatarChromeFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AvatarChromeFrame")
            .field("shape", &self.shape)
            .finish()
    }
}

impl Widget for AvatarChromeFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Repaint when focus changes — the ring appears / disappears.
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_focused
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Fill the parent's proposal — the parent `Avatar` widget owns
        // the size policy (`avatar_pixel_size(self.size)`).
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
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

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;

        // Background fill — shape-aware. Default is hash-picked from
        // the chart palette using `seed`.
        let bg = match &self.background {
            Some(prop) => prop.resolve(theme),
            None => hash_pick_palette_color(&self.seed, theme),
        };
        match self.shape {
            AvatarShape::Circle => {
                let radius = bounds.width.min(bounds.height) / 2.0;
                let center = Point::new(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                );
                canvas.fill_circle(center, radius, bg);
            }
            AvatarShape::RoundedSquare => {
                let r = bounds.width.min(bounds.height) * AVATAR_ROUNDED_RADIUS_RATIO;
                canvas.fill_rounded_rect(bounds, CornerRadius::uniform(r), bg);
            }
            AvatarShape::Square => {
                canvas.fill_rounded_rect(bounds, CornerRadius::uniform(0.0), bg);
            }
        }

        // Border (outer ring) — drawn over content to mask image
        // bleed. Half-stroke inset so the ring sits inside `bounds`.
        if let Some(width) = self.border_width
            && width > 0.0
        {
            let color = match &self.border_color {
                Some(prop) => prop.resolve(theme),
                None => theme.colors.surface_main,
            };
            paint_border(
                canvas,
                bounds,
                self.shape,
                AVATAR_ROUNDED_RADIUS_RATIO,
                width,
                color,
            );
        }

        // Focus ring — outside the avatar bounds, hugging the shape.
        if self.is_focused.get() {
            paint_focus_ring(
                canvas,
                bounds,
                self.shape,
                AVATAR_ROUNDED_RADIUS_RATIO,
                theme.shape.focus_ring_offset,
                theme.shape.focus_ring_width,
                theme.colors.focus_ring,
            );
        }

        // Presence dot — on top of everything, with a surface_main
        // outline that "punches" it out of the avatar.
        if let Some(presence) = &self.presence {
            let color = presence.color(theme);
            let dot_diameter = (bounds.width.min(bounds.height) * AVATAR_PRESENCE_DIAMETER_RATIO)
                .clamp(AVATAR_PRESENCE_DIAMETER_MIN, AVATAR_PRESENCE_DIAMETER_MAX);
            let dot_radius = dot_diameter / 2.0;
            let (xf, yf) = self.presence_corner.offset();
            let cx = if xf < 0.0 {
                bounds.x + dot_radius + AVATAR_PRESENCE_INSET
            } else {
                bounds.x + bounds.width - dot_radius - AVATAR_PRESENCE_INSET
            };
            let cy = if yf < 0.0 {
                bounds.y + dot_radius + AVATAR_PRESENCE_INSET
            } else {
                bounds.y + bounds.height - dot_radius - AVATAR_PRESENCE_INSET
            };
            let center = Point::new(cx, cy);
            let outline_radius = dot_radius + AVATAR_PRESENCE_OUTLINE_WIDTH;
            canvas.fill_circle(center, outline_radius, theme.colors.surface_main);
            canvas.fill_circle(center, dot_radius, color);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent `Avatar` emits the user-facing
        // Image / Label / Button node.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
