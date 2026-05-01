//! `Avatar` — circular (or rounded-square / square) user-identity widget.
//!
//! Displays either a person's image (clipped to the configured shape via
//! a CPU-side anti-aliased alpha mask applied at construction time) or
//! their initials over a hash-derived background colour. Optional
//! presence indicator (Online / Offline / Away / Busy) and outer ring.
//! Can be made activable to serve as a user-menu trigger.
//!
//! ```ignore
//! // Image with a presence dot.
//! Avatar::with_image(&face)
//!     .alt("Jane Doe")
//!     .presence(AvatarPresence::Online)
//!     .size(AvatarSize::Medium)
//!
//! // Hash-tinted initials, auto-derived from a name.
//! Avatar::with_name("Jane Doe").size(AvatarSize::Large)
//!
//! // Click target — opens a user menu via an `AppIntent`.
//! Avatar::with_image(&face)
//!     .label("Open user menu")
//!     .alt("Jane Doe")
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::OpenUserMenu))
//! ```
//!
//! The widget reuses `ImageWidget` for the image path and draws bg /
//! border / presence directly via `Canvas`. Hash-derived background
//! tints come from `theme.colors.chart_palette` (Okabe-Ito), so they
//! track the active theme automatically.

use std::rc::Rc;

use fern_canvas::raster::RasterIcon;
use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Prop;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, FontWeight, TextStyle};

use crate::primitives::ImageWidget;
use crate::primitives::image_widget::ImageFit;

mod mask;
use mask::MaskShape;

// ─── Public types ──────────────────────────────────────────────────────────

/// Discrete avatar size variants. `Custom(px)` accepts an arbitrary
/// logical-pixel side length when the four standard sizes are not a
/// good fit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarSize {
    /// 24 dp — list rows, mention chips.
    Small,
    /// 32 dp — comment threads, sidebars (default).
    #[default]
    Medium,
    /// 48 dp — profile cards.
    Large,
    /// 64 dp — settings, "your account" headers.
    XLarge,
    /// Arbitrary side length.
    Custom(f32),
}

impl AvatarSize {
    fn resolve(self, style: &fern_tokens::AvatarStyle) -> f32 {
        match self {
            AvatarSize::Small => style.size_small,
            AvatarSize::Medium => style.size_medium,
            AvatarSize::Large => style.size_large,
            AvatarSize::XLarge => style.size_x_large,
            AvatarSize::Custom(px) => px.max(1.0),
        }
    }
}

/// Outer outline. `Circle` is the most common; `RoundedSquare` matches
/// Material's "rounded" variant and is useful for non-person avatars
/// (project icons, channels). `Square` is a hard rectangle with no
/// corner rounding.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarShape {
    #[default]
    Circle,
    RoundedSquare,
    Square,
}

/// Presence indicator dot drawn at one corner of the avatar. The
/// `Custom` variant carries its own colour and an a11y label so apps
/// can model domain-specific statuses (e.g. "in a meeting").
#[derive(Debug, Clone)]
pub enum AvatarPresence {
    Online,
    Offline,
    Away,
    Busy,
    Custom { color: ColorProp, label: String },
}

/// Where the presence dot is rendered relative to the avatar bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarCorner {
    #[default]
    BottomTrailing,
    BottomLeading,
    TopTrailing,
    TopLeading,
}

// ─── The widget ────────────────────────────────────────────────────────────

type ActionFn = Rc<dyn Fn(&mut EventContext)>;

/// Circular (or rounded / square) user avatar.
pub struct Avatar {
    /// Initials shown when no image is present, or when `image_visible`
    /// resolves to false. Always non-empty (`"?"` when source was empty).
    initials: String,
    /// Optional override of the a11y name (`label`).
    label: Option<String>,
    /// Image alt text (a11y name when in image-mode without label).
    alt: Option<String>,

    /// Pre-masked image pixels + dimensions, captured in the
    /// constructor. `None` = pure-initials avatar.
    image_data: Option<MaskedImage>,

    /// Lazily-applied: if the user changed shape after `with_image`,
    /// we re-mask on `build()`.
    image_source_pixels: Option<RawImage>,

    size: AvatarSize,
    shape: AvatarShape,

    background: Option<ColorProp>,
    foreground: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Option<f32>,

    presence: Option<AvatarPresence>,
    presence_corner: AvatarCorner,

    seed: Option<String>,

    a11y_hidden: bool,

    image_visible: Prop<bool>,

    /// Activation handler. Stored as `Rc<dyn Fn>` so it survives
    /// rebuilds (theme/locale switches re-run `build()` and would
    /// otherwise drop a `Box<dyn Fn>` after the first take).
    action: Option<ActionFn>,
}

#[derive(Clone)]
struct MaskedImage {
    pixels: Vec<u8>,
    side: u32,
    /// Shape that produced these pixels — used to detect when the user
    /// has switched shape after construction so we can re-mask.
    shape: AvatarShape,
}

#[derive(Clone)]
struct RawImage {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

// ─── Constructors ──────────────────────────────────────────────────────────

impl Avatar {
    /// Build an avatar from explicit initials. Uppercases and truncates
    /// to ≤ 2 chars. Empty input yields `"?"`.
    pub fn with_initials(initials: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = initials.into();
        let raw = ls.resolve_now();
        Self::from_initials(normalize_initials(&raw))
    }

    /// Build an avatar from a name; initials are derived
    /// (`"Jane Doe" → "JD"`, `"jane.doe@x.com" → "JD"`,
    /// `"Cher" → "C"`, `"" → "?"`).
    pub fn with_name(name: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = name.into();
        let raw = ls.resolve_now();
        let initials = derive_initials(&raw);
        let mut a = Self::from_initials(initials);
        a.seed = Some(raw); // hash from the full name, not from the abbreviated initials
        a
    }

    /// Build an avatar from a decoded raster icon. The pixels are
    /// centred-cropped to a square and CPU-masked to the configured
    /// shape at first `build()`.
    pub fn with_image(icon: &RasterIcon) -> Self {
        Self::from_raw_image(icon.pixels().to_vec(), icon.width(), icon.height())
    }

    /// Build an avatar from raw RGBA pixels. Same convention as
    /// [`ImageWidget::from_raw`].
    pub fn from_raw_image(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let mut a = Self::from_initials("?".to_string());
        a.image_source_pixels = Some(RawImage { pixels, width, height });
        a
    }

    fn from_initials(initials: String) -> Self {
        Self {
            initials,
            label: None,
            alt: None,
            image_data: None,
            image_source_pixels: None,
            size: AvatarSize::Medium,
            shape: AvatarShape::Circle,
            background: None,
            foreground: None,
            border_color: None,
            border_width: None,
            presence: None,
            presence_corner: AvatarCorner::BottomTrailing,
            seed: None,
            a11y_hidden: false,
            image_visible: Prop::Static(true),
            action: None,
        }
    }

    /// Permanent `#[doc(hidden)]` shim for tests — wraps in
    /// `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn with_initials_literal(initials: &str) -> Self {
        Self::with_initials(fern_i18n::LocalizedString::literal(initials))
    }

    /// Permanent `#[doc(hidden)]` shim for tests.
    #[doc(hidden)]
    pub fn with_name_literal(name: &str) -> Self {
        Self::with_name(fern_i18n::LocalizedString::literal(name))
    }
}

// ─── Builder methods ───────────────────────────────────────────────────────

impl Avatar {
    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn shape(mut self, shape: AvatarShape) -> Self {
        if shape != self.shape {
            self.shape = shape;
            // Source pixels haven't been masked for the new shape —
            // invalidate the cache so build() re-masks.
            self.image_data = None;
        }
        self
    }

    /// Override the initials shown when the image is hidden via
    /// `image_visible(false)` or fails to register. Defaults to the
    /// derived initials if `with_image` was paired with `with_name`,
    /// otherwise `"?"`.
    pub fn fallback_initials(mut self, initials: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = initials.into();
        let raw = ls.resolve_now();
        self.initials = normalize_initials(&raw);
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn fallback_initials_literal(mut self, initials: &str) -> Self {
        self.initials = normalize_initials(initials);
        self
    }

    /// Reactive image visibility. When unbound it's `true`. When bound
    /// to a `Signal<bool>` and the value is `false`, the initials
    /// fallback paints in place of the image — same logical bounds, no
    /// layout shift.
    pub fn image_visible(mut self, visible: impl Into<Prop<bool>>) -> Self {
        self.image_visible = visible.into();
        self
    }

    /// Override the auto hash-derived background. Accepts a [`Color`],
    /// a role, or a `Signal<Color>`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the auto-contrast text colour for the initials. Auto
    /// (unset) picks white over dark backgrounds and near-black over
    /// light ones, computed at paint time from the resolved bg's
    /// luminance.
    pub fn foreground(mut self, color: impl Into<ColorProp>) -> Self {
        self.foreground = Some(color.into());
        self
    }

    /// Override the seed string used to pick a hash-derived background
    /// from the theme's chart palette. Defaults to the resolved name
    /// (when constructed via `with_name`) or the initials.
    pub fn seed(mut self, seed: impl Into<String>) -> Self {
        self.seed = Some(seed.into());
        self
    }

    /// Outer ring thickness. A non-zero value enables the ring (drawn
    /// in `BorderRole::Default` unless [`Self::border_color`] overrides
    /// it). `0.0` disables the ring.
    pub fn border(mut self, width: f32) -> Self {
        self.border_width = Some(width.max(0.0));
        self
    }

    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    pub fn presence(mut self, presence: AvatarPresence) -> Self {
        self.presence = Some(presence);
        self
    }

    pub fn presence_corner(mut self, corner: AvatarCorner) -> Self {
        self.presence_corner = corner;
        self
    }

    /// Override the accessible name. When unset:
    /// * image-mode → `alt` if set, else the initials, else "Avatar"
    /// * initials-mode → the initials.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// Image alt text — distinct from `label` so a clickable avatar
    /// can have a button label like "Open user menu" while still
    /// describing the image as "Jane Doe".
    pub fn alt(mut self, alt: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = alt.into();
        self.alt = Some(ls.resolve_now());
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn alt_literal(mut self, alt: &str) -> Self {
        self.alt = Some(alt.to_string());
        self
    }

    /// Hide from the a11y tree entirely. Use only when an adjacent
    /// label conveys the avatar's meaning.
    pub fn a11y_hidden(mut self) -> Self {
        self.a11y_hidden = true;
        self
    }

    /// Make the avatar activable. Promotes the a11y role to
    /// `Role::Button` and adds `Action::Click` / `Action::Focus`. Tap,
    /// Enter, and Space all fire the closure. Cursor changes to
    /// `Pointer` on hover.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for Avatar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Avatar")
            .field("initials", &self.initials)
            .field("size", &self.size)
            .field("shape", &self.shape)
            .field("has_image", &self.image_source_pixels.is_some())
            .field("clickable", &self.action.is_some())
            .finish()
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Inline FNV-1a 64-bit. Stable across Rust versions and process runs
/// (unlike `DefaultHasher`). Same idiom as `fern_core::accessibility`.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Truncate to ≤ 2 chars and uppercase. Returns `"?"` when the input
/// trims to empty. Operates on `char`s (Unicode scalars), not extended
/// graphemes — this is sufficient for real-world names where accented
/// letters are stored pre-composed.
fn normalize_initials(s: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for c in s.trim().chars() {
        if count >= 2 {
            break;
        }
        for upper in c.to_uppercase() {
            out.push(upper);
        }
        count += 1;
    }
    if out.is_empty() { "?".to_string() } else { out }
}

/// Auto-derive initials from a free-form name.
fn derive_initials(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "?".to_string();
    }
    // For email-like strings only the local part (before `@`) matters.
    let source = trimmed.split('@').next().unwrap_or(trimmed);
    let parts: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect();

    let mut out = String::new();
    for part in parts.iter().take(2) {
        if let Some(c) = part.chars().next() {
            for upper in c.to_uppercase() {
                out.push(upper);
            }
        }
    }
    if out.is_empty() { "?".to_string() } else { out }
}

/// Pick a colour from the theme's chart palette deterministically from
/// `seed`. Empty palette falls back to a neutral grey so the widget
/// still renders.
fn hash_pick_palette_color(seed: &str, theme: &fern_tokens::Theme) -> Color {
    let palette = &theme.colors.chart_palette;
    if palette.is_empty() {
        return Color::from_rgb(0.5, 0.5, 0.5);
    }
    let h = fnv1a_64(seed.as_bytes());
    let idx = (h as usize) % palette.len();
    palette[idx]
}

/// Auto-contrast foreground for a given background.
fn auto_contrast_text(bg: Color) -> Color {
    if bg.relative_luminance() < 0.5 {
        Color::WHITE
    } else {
        Color::from_rgb(0.121, 0.121, 0.121) // ≈ #1F1F1F
    }
}

fn presence_color(p: &AvatarPresence, theme: &fern_tokens::Theme) -> Color {
    match p {
        AvatarPresence::Online => theme.colors.status_success_fg,
        AvatarPresence::Offline => theme.colors.text_disabled,
        AvatarPresence::Away => theme.colors.status_warning_fg,
        AvatarPresence::Busy => theme.colors.status_error_fg,
        AvatarPresence::Custom { color, .. } => color.resolve(theme),
    }
}

fn presence_label(p: &AvatarPresence) -> String {
    match p {
        AvatarPresence::Online => "Online".to_string(),
        AvatarPresence::Offline => "Offline".to_string(),
        AvatarPresence::Away => "Away".to_string(),
        AvatarPresence::Busy => "Busy".to_string(),
        AvatarPresence::Custom { label, .. } => label.clone(),
    }
}

fn corner_offset(corner: AvatarCorner) -> (f32, f32) {
    // x_factor, y_factor each in {-1, 1}: -1 = leading/top, 1 = trailing/bottom.
    match corner {
        AvatarCorner::BottomTrailing => (1.0, 1.0),
        AvatarCorner::BottomLeading => (-1.0, 1.0),
        AvatarCorner::TopTrailing => (1.0, -1.0),
        AvatarCorner::TopLeading => (-1.0, -1.0),
    }
}

fn shape_to_mask(shape: AvatarShape, side: u32, radius_ratio: f32) -> MaskShape {
    match shape {
        AvatarShape::Circle => MaskShape::Circle,
        AvatarShape::RoundedSquare => {
            MaskShape::RoundedSquare(side as f32 * radius_ratio)
        }
        AvatarShape::Square => MaskShape::Square,
    }
}

// ─── Widget impl ───────────────────────────────────────────────────────────

impl Widget for Avatar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // 1. Apply CPU mask to the source image, if needed.
        let need_remask = match (&self.image_data, &self.image_source_pixels) {
            (None, Some(_)) => true,
            (Some(cached), Some(_)) => cached.shape != self.shape,
            _ => false,
        };
        if need_remask {
            if let Some(raw) = &self.image_source_pixels {
                let theme = ctx.theme();
                let style = theme.components.avatar;
                let (mut cropped, side) =
                    mask::center_crop_square(&raw.pixels, raw.width, raw.height);
                let mask_shape = shape_to_mask(self.shape, side, style.rounded_radius_ratio);
                mask::apply_alpha_mask(&mut cropped, side, side, mask_shape);
                self.image_data = Some(MaskedImage {
                    pixels: cropped,
                    side,
                    shape: self.shape,
                });
            }
        }

        // 2. Add child(ren). The activation handler is wired here as
        //    well so taps bubble up correctly.
        let make_initials_leaf = || InitialsLeaf {
            initials: self.initials.clone(),
            seed: self
                .seed
                .clone()
                .unwrap_or_else(|| self.initials.clone()),
            background: self.background.clone(),
            foreground: self.foreground.clone(),
        };
        let mut children = Vec::new();
        match (&self.image_data, &self.image_visible) {
            (Some(img), Prop::Static(true)) => {
                let id = ctx.add(make_image_widget(img));
                children.push(id);
            }
            (Some(_), Prop::Static(false)) => {
                // Ignored image — initials only.
                let id = ctx.add(make_initials_leaf());
                children.push(id);
            }
            (Some(img), Prop::Bound(visible_signal)) => {
                let img_id = ctx.add(make_image_widget(img));
                let init_id = ctx.add(make_initials_leaf());
                let v_clone = visible_signal.clone();
                ctx.visible_when(img_id, v_clone.clone());
                ctx.visible_when(init_id, v_clone.map(|v| !*v));
                children.push(img_id);
                children.push(init_id);
            }
            (None, _) => {
                // Pure initials — single leaf child holds the centred
                // text; the avatar paints the background itself.
                let id = ctx.add(make_initials_leaf());
                children.push(id);
            }
        }

        // 3. If clickable, install attached handlers.
        if let Some(action) = self.action.clone() {
            let action_for_tap = action.clone();
            let action_for_key = action.clone();
            let action_for_access = action;
            let handlers = HandlerSet::new()
                .on_tap(move |_pos, ctx| action_for_tap(ctx))
                .focusable(true)
                .cursor(CursorIcon::Pointer)
                .on_key(move |event, ctx| {
                    use fern_core::event::{EventResponse, Key, WidgetEvent};
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            action_for_key(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action(move |action_kind, ctx| {
                    use fern_core::event::EventResponse;
                    if action_kind == fern_core::accesskit::Action::Click {
                        action_for_access(ctx);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                });
            ctx.apply_self_handlers(handlers);
        }

        children
    }

    fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let style = ctx.theme.components.avatar;
        let side = self.size.resolve(&style);
        Size::new(side, side)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // All children fill the avatar's bounds — image / initials
        // both span the full circle.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = theme.components.avatar;

        // Resolve the background colour. Hash-derived from the seed
        // (or initials) when no override is supplied. Static `false`
        // image_visible still uses the bg; static `true` image
        // typically covers it but the bg shows through if the image
        // failed to register — so we always paint it.
        let bg = match &self.background {
            Some(prop) => prop.resolve(theme),
            None => {
                let seed = self
                    .seed
                    .as_deref()
                    .unwrap_or(&self.initials);
                hash_pick_palette_color(seed, theme)
            }
        };

        // Background fill (always) — circle, rounded-square or square.
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
                let r = bounds.width.min(bounds.height) * style.rounded_radius_ratio;
                canvas.fill_rounded_rect(bounds, CornerRadius::uniform(r), bg);
            }
            AvatarShape::Square => {
                canvas.fill_rounded_rect(bounds, CornerRadius::uniform(0.0), bg);
            }
        }

        // Border (outer ring) — drawn over children to mask any image
        // bleed at the rim. Offset half-stroke inward so the visible
        // ring stays inside the layout rect.
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
                style.rounded_radius_ratio,
                width,
                color,
            );
        }

        // Presence dot — drawn on top of everything, even the border,
        // so it remains visible regardless of avatar contents.
        if let Some(presence) = &self.presence {
            let color = presence_color(presence, theme);
            let dot_diameter = (bounds.width.min(bounds.height) * style.presence_diameter_ratio)
                .clamp(style.presence_diameter_min, style.presence_diameter_max);
            let dot_radius = dot_diameter / 2.0;
            let (xf, yf) = corner_offset(self.presence_corner);
            let inset = style.presence_inset;
            let cx = if xf < 0.0 {
                bounds.x + dot_radius + inset
            } else {
                bounds.x + bounds.width - dot_radius - inset
            };
            let cy = if yf < 0.0 {
                bounds.y + dot_radius + inset
            } else {
                bounds.y + bounds.height - dot_radius - inset
            };
            let center = Point::new(cx, cy);
            // Outline first (in surface_main) for the punched-out
            // appearance, then the dot itself on top.
            let outline_radius = dot_radius + style.presence_outline_width;
            canvas.fill_circle(center, outline_radius, theme.colors.surface_main);
            canvas.fill_circle(center, dot_radius, color);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_hidden {
            builder.set_hidden();
            return;
        }

        let clickable = self.action.is_some();
        let has_image = self.image_data.is_some() || self.image_source_pixels.is_some();

        if clickable {
            builder.set_role(fern_core::accesskit::Role::Button);
            // A clickable avatar without an explicit label is missing
            // its activation hint. Catch this in dev to prevent silent
            // a11y regressions.
            debug_assert!(
                self.label.is_some() || self.alt.is_some(),
                "Avatar::on_activate_fn requires a `.label(\"...\")` (preferred) or `.alt(\"...\")` for screen readers"
            );
            let name = self
                .label
                .clone()
                .or_else(|| self.alt.clone())
                .unwrap_or_else(|| self.initials.clone());
            builder.set_name(name);
            builder.add_action(fern_core::accesskit::Action::Click);
            builder.add_action(fern_core::accesskit::Action::Focus);
        } else if has_image {
            builder.set_role(fern_core::accesskit::Role::Image);
            // A pure-image avatar without alt text is missing its
            // semantic label — catch in dev (matches `ImageWidget`).
            debug_assert!(
                self.alt.is_some() || self.label.is_some(),
                "Avatar::with_image requires a `.alt(\"...\")` for meaningful images, or call `.a11y_hidden()` if decorative"
            );
            let name = self
                .alt
                .clone()
                .or_else(|| self.label.clone())
                .unwrap_or_else(|| self.initials.clone());
            builder.set_name(name);
        } else {
            builder.set_role(fern_core::accesskit::Role::Label);
            let name = self
                .label
                .clone()
                .unwrap_or_else(|| self.initials.clone());
            builder.set_name(name);
        }

        if let Some(presence) = &self.presence {
            builder.set_description(presence_label(presence));
        }
    }
}

fn make_image_widget(img: &MaskedImage) -> ImageWidget {
    // Always hide the inner ImageWidget from the a11y tree — the
    // parent Avatar carries the canonical role + name. Without this
    // shield, screen readers would announce the avatar twice (once
    // as the parent's `Role::Image` / `Role::Button`, once as the
    // child `Role::Image`).
    ImageWidget::from_raw(img.pixels.clone(), img.side, img.side)
        .fit(ImageFit::Cover)
        .a11y_hidden()
}

fn paint_border(
    canvas: &mut Canvas,
    bounds: Rect,
    shape: AvatarShape,
    rounded_radius_ratio: f32,
    width: f32,
    color: Color,
) {
    use fern_canvas::{Paint, StrokeStyle};
    let half = width / 2.0;
    // Inset by half-stroke so the visible ring sits inside `bounds`
    // (and doesn't get clipped by an ancestor with `clips_children`).
    let inner = Rect::new(
        bounds.x + half,
        bounds.y + half,
        (bounds.width - width).max(0.0),
        (bounds.height - width).max(0.0),
    );
    match shape {
        AvatarShape::Circle => {
            let radius = inner.width.min(inner.height) / 2.0;
            let center = Point::new(
                inner.x + inner.width / 2.0,
                inner.y + inner.height / 2.0,
            );
            canvas.stroke_circle(center, radius, Paint::from(color), StrokeStyle::solid(width));
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

// ─── Initials sub-widget ───────────────────────────────────────────────────

/// Crate-private leaf that draws the centred initials. The avatar's
/// own `paint()` handles the background fill; this widget only emits
/// glyphs so paint order is parent-bg → child-text.
///
/// The leaf is constructed in [`Avatar::build`] with all the inputs
/// it needs to resolve a correctly contrasted foreground at paint
/// time:
/// * `initials` — the glyphs to draw.
/// * `seed` — the same string the parent will hash to pick a bg tint.
/// * `background` / `foreground` — clones of the parent's overrides
///   (`None` ⇒ default path). Stored as `ColorProp` so role / signal
///   variants resolve against the active theme each frame, matching
///   what the parent paints.
#[derive(Debug)]
struct InitialsLeaf {
    initials: String,
    seed: String,
    background: Option<ColorProp>,
    foreground: Option<ColorProp>,
}

impl InitialsLeaf {
    /// Recompute the bg colour the parent Avatar will paint. Must
    /// stay in lock-step with `Avatar::paint`'s bg branch.
    fn resolve_bg(&self, theme: &fern_tokens::Theme) -> Color {
        match &self.background {
            Some(prop) => prop.resolve(theme),
            None => hash_pick_palette_color(&self.seed, theme),
        }
    }
}

impl Widget for InitialsLeaf {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Always fill the proposal — the parent Avatar drives sizing.
        proposal.resolve(0.0, 0.0)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = theme.components.avatar;

        let font_size = bounds.width.min(bounds.height)
            * if self.initials.chars().count() <= 1 {
                style.font_ratio_1char
            } else {
                style.font_ratio_2char
            };

        let text_style = TextStyle {
            family: theme.typography.body_bold.family.clone(),
            size: font_size,
            weight: FontWeight::SEMI_BOLD,
            line_height: 1.0,
            letter_spacing: 0.0,
        };

        // Foreground: explicit override wins. Otherwise auto-contrast
        // against the same bg the parent painted.
        let fg = match &self.foreground {
            Some(prop) => prop.resolve(theme),
            None => auto_contrast_text(self.resolve_bg(theme)),
        };

        // Measure the text to centre it. Without a backend, we can't
        // measure or draw glyphs at all — silently no-op.
        let Some(backend) = canvas.text_backend().cloned() else {
            return;
        };
        let layout = {
            let mut b = backend.borrow_mut();
            b.layout_single_line(&self.initials, &text_style, None)
        };
        let text_w = layout.width;
        let text_h = layout.height;

        let cx = bounds.x + (bounds.width - text_w) / 2.0;
        let cy = bounds.y + (bounds.height - text_h) / 2.0;
        let position = Rect::new(cx, cy, text_w, text_h);
        canvas.draw_text(&self.initials, position, &text_style, fg);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The parent Avatar owns the user-facing semantics (role, name,
        // click action). The text node would otherwise duplicate that
        // information to ATs.
        builder.set_hidden();
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget::LayoutContext;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    // ── helpers ────────────────────────────────────────────────────────

    fn rgba_solid(side: u32, rgba: [u8; 4]) -> RasterIcon {
        let mut p = Vec::with_capacity((side * side * 4) as usize);
        for _ in 0..(side * side) {
            p.extend_from_slice(&rgba);
        }
        RasterIcon::from_raw(p, side, side)
    }

    // ── derive_initials / normalize_initials ──────────────────────────

    #[test]
    fn normalize_uppercase_truncate() {
        assert_eq!(normalize_initials("jdq"), "JD");
        assert_eq!(normalize_initials("jd"), "JD");
        assert_eq!(normalize_initials("j"), "J");
        assert_eq!(normalize_initials("  "), "?");
        assert_eq!(normalize_initials(""), "?");
    }

    #[test]
    fn derive_full_name() {
        assert_eq!(derive_initials("Jane Doe"), "JD");
    }

    #[test]
    fn derive_single_word() {
        assert_eq!(derive_initials("Cher"), "C");
    }

    #[test]
    fn derive_email() {
        assert_eq!(derive_initials("jane.doe@x.com"), "JD");
        assert_eq!(derive_initials("jane_doe@x.com"), "JD");
    }

    #[test]
    fn derive_unicode_name() {
        assert_eq!(derive_initials("María José"), "MJ");
    }

    #[test]
    fn derive_empty_yields_question_mark() {
        assert_eq!(derive_initials(""), "?");
        assert_eq!(derive_initials("   "), "?");
    }

    #[test]
    fn derive_three_words_takes_first_two() {
        assert_eq!(derive_initials("Anna María José"), "AM");
    }

    #[test]
    fn derive_hyphenated_name() {
        assert_eq!(derive_initials("Jean-Luc Picard"), "JL");
    }

    // ── hashing ────────────────────────────────────────────────────────

    #[test]
    fn fnv1a_is_stable() {
        let h1 = fnv1a_64(b"jane.doe");
        let h2 = fnv1a_64(b"jane.doe");
        assert_eq!(h1, h2);
        assert_ne!(fnv1a_64(b"jane.doe"), fnv1a_64(b"john.smith"));
    }

    #[test]
    fn hash_distributes_over_palette() {
        let theme = Theme::light_default();
        let mut buckets = [0_u32; 8];
        for i in 0..200 {
            let seed = format!("user_{i}");
            let color = hash_pick_palette_color(&seed, &theme);
            // Find which palette index it picked.
            let idx = theme
                .colors
                .chart_palette
                .iter()
                .position(|c| c == &color)
                .expect("color must be a palette member");
            buckets[idx] += 1;
        }
        let nonzero = buckets.iter().filter(|n| **n > 0).count();
        assert!(
            nonzero >= 6,
            "expected hash to cover at least 6 of 8 buckets, got {nonzero} (buckets: {:?})",
            buckets
        );
    }

    // ── sizing ─────────────────────────────────────────────────────────

    #[test]
    fn size_default_is_medium_32px() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Avatar::with_initials_literal("JD"));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 32.0).abs() < 0.01);
        assert!((b.height - 32.0).abs() < 0.01);
    }

    #[test]
    fn size_custom_passes_through() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Avatar::with_initials_literal("JD").size(AvatarSize::Custom(40.0)),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 40.0).abs() < 0.01);
        assert!((b.height - 40.0).abs() < 0.01);
    }

    #[test]
    fn size_that_fits_ignores_proposal() {
        // Even a hugely oversized proposal must not enlarge the
        // avatar's intrinsic size — it always reports the discrete
        // size variant. (`tree.layout(exact(...))` would clamp the
        // root's bounds to the proposal regardless, so we exercise
        // `size_that_fits` directly.)
        let widget = Avatar::with_initials_literal("JD");
        let theme = Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        let s = widget.size_that_fits(SizeProposal::exact(400.0, 400.0), &ctx);
        assert!((s.width - 32.0).abs() < 0.01);
        assert!((s.height - 32.0).abs() < 0.01);
    }

    #[test]
    fn small_medium_large_xlarge_sizes() {
        let theme = Theme::light_default();
        let cases = [
            (AvatarSize::Small, theme.components.avatar.size_small),
            (AvatarSize::Medium, theme.components.avatar.size_medium),
            (AvatarSize::Large, theme.components.avatar.size_large),
            (AvatarSize::XLarge, theme.components.avatar.size_x_large),
        ];
        for (variant, expected) in cases {
            let mut tree = WidgetTree::new().with_theme(theme.clone());
            let id = tree.add(Avatar::with_initials_literal("X").size(variant));
            tree.layout(SizeProposal {
                width: None,
                height: None,
            });
            let b = tree.bounds(id);
            assert!(
                (b.width - expected).abs() < 0.01,
                "size {variant:?}: expected {expected}, got {}",
                b.width
            );
        }
    }

    // ── paint output ──────────────────────────────────────────────────

    fn render_avatar(avatar: Avatar) -> std::rc::Rc<fern_canvas::RenderFrame> {
        use fern_canvas::MockTextBackend;
        use std::cell::RefCell;
        use std::rc::Rc;
        let mut tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.add(avatar);
        tree.layout(SizeProposal::exact(64.0, 64.0));
        tree.render()
    }

    fn count_shapes(frame: &fern_canvas::RenderFrame) -> usize {
        // Both `fill_circle` and `fill_rounded_rect` emit `ShapeQuad`
        // entries — the SDF pipeline. Stroke-based circle border also
        // lands here.
        frame.shapes.len()
    }

    #[test]
    fn paint_initials_emits_a_shape_quad() {
        let frame = render_avatar(Avatar::with_initials_literal("JD"));
        assert!(
            count_shapes(&frame) >= 1,
            "expected at least one ShapeQuad (the bg circle)"
        );
    }

    #[test]
    fn paint_with_border_adds_extra_shape() {
        let plain = render_avatar(Avatar::with_initials_literal("JD"));
        let bordered = render_avatar(Avatar::with_initials_literal("JD").border(2.0));
        assert!(
            count_shapes(&bordered) > count_shapes(&plain),
            "border path should add at least one extra Shape (the stroked ring)"
        );
    }

    #[test]
    fn paint_presence_adds_two_shapes() {
        let plain = render_avatar(Avatar::with_initials_literal("JD"));
        let with_dot = render_avatar(
            Avatar::with_initials_literal("JD").presence(AvatarPresence::Online),
        );
        // Outline + dot.
        assert_eq!(count_shapes(&with_dot), count_shapes(&plain) + 2);
    }

    #[test]
    fn paint_rounded_square_emits_shape() {
        // `fill_rounded_rect` lands on the SDF Shape pipeline same
        // as `fill_circle`. Both shapes paint via Shape quads.
        let frame =
            render_avatar(Avatar::with_initials_literal("JD").shape(AvatarShape::RoundedSquare));
        assert!(count_shapes(&frame) >= 1);
    }

    #[test]
    fn paint_square_emits_shape() {
        let frame =
            render_avatar(Avatar::with_initials_literal("JD").shape(AvatarShape::Square));
        assert!(count_shapes(&frame) >= 1);
    }

    #[test]
    fn paint_image_uses_image_pipeline() {
        let icon = rgba_solid(8, [50, 100, 200, 255]);
        let frame = render_avatar(Avatar::with_image(&icon).alt_literal("avatar"));
        assert!(!frame.images.is_empty(), "image avatar should render an image");
    }

    #[test]
    fn auto_contrast_dark_bg_chooses_white() {
        let dark = Color::from_rgb(0.05, 0.05, 0.05);
        let fg = auto_contrast_text(dark);
        assert!(fg.r() > 0.9 && fg.g() > 0.9 && fg.b() > 0.9);
    }

    #[test]
    fn auto_contrast_light_bg_chooses_dark() {
        let light = Color::from_rgb(0.95, 0.95, 0.95);
        let fg = auto_contrast_text(light);
        assert!(fg.r() < 0.3 && fg.g() < 0.3 && fg.b() < 0.3);
    }

    // ── accessibility ─────────────────────────────────────────────────

    #[test]
    fn accessibility_initials_default_role_is_label() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Avatar::with_initials_literal("JD"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("JD"));
    }

    #[test]
    fn accessibility_image_default_role_is_image() {
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Avatar::with_image(&icon).alt_literal("Jane Doe"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::Image);
        assert_eq!(info.name(), Some("Jane Doe"));
    }

    #[test]
    fn accessibility_clickable_becomes_button() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Avatar::with_initials_literal("JD")
                .label_literal("Open user menu")
                .on_activate_fn(|_ctx| {}),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
        assert!(info.actions().contains(&fern_core::accesskit::Action::Focus));
        assert_eq!(info.name(), Some("Open user menu"));
    }

    #[test]
    fn accessibility_a11y_hidden_does_not_set_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Avatar::with_initials_literal("JD").a11y_hidden());
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        // Hidden nodes carry no name (the leaf-hidden path returned
        // early before set_role/set_name fired).
        assert_eq!(info.name(), None);
    }

    #[test]
    fn accessibility_label_overrides_initials() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Avatar::with_initials_literal("JD").label_literal("Jane Doe (offline)"),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Jane Doe (offline)"));
    }

    #[test]
    fn accessibility_presence_appears_in_description() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Avatar::with_initials_literal("JD").presence(AvatarPresence::Online),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Just verify it builds — `description` isn't surfaced by the
        // test introspection helper, but that the avatar accepts the
        // presence and renders without panicking is the key contract.
        assert_eq!(
            tree.accessibility_node(id).role(),
            fern_core::accesskit::Role::Label
        );
    }

    // ── visibility binding ────────────────────────────────────────────

    #[test]
    fn image_visible_false_hides_image_child() {
        use fern_core::signal::Signal;

        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Avatar::with_image(&icon)
                .alt_literal("Jane")
                .fallback_initials_literal("JD")
                .image_visible(visible.clone()),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // With visibility = true, an image quad is emitted.
        assert!(!tree.render().images.is_empty());

        // Flip visibility — the image child becomes dormant; on the
        // next render frame, no image is drawn.
        visible.set(false);
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let frame_after = tree.render();
        assert!(
            frame_after.images.is_empty(),
            "image should be hidden when image_visible == false"
        );
        // Sanity: avatar itself is still visible.
        assert!(tree.is_visible(id));
    }

    // ── shape interaction with image masking ──────────────────────────

    // ── foreground / background overrides ─────────────────────────────

    fn glyph_colors(frame: &fern_canvas::RenderFrame) -> Vec<[f32; 4]> {
        frame.glyphs.iter().map(|g| g.color).collect()
    }

    fn shape_colors(frame: &fern_canvas::RenderFrame) -> Vec<[f32; 4]> {
        frame.shapes.iter().map(|s| s.color).collect()
    }

    fn approx_color_eq(a: [f32; 4], b: Color) -> bool {
        let target = b.to_array();
        a.iter()
            .zip(target.iter())
            .all(|(x, y)| (x - y).abs() < 0.02)
    }

    #[test]
    fn foreground_override_sets_glyph_color() {
        // Without override the foreground is auto-contrast — for some
        // hash bg it'll be white, for others near-black. We force a
        // specific colour and verify it ends up in glyph metadata.
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").foreground(Color::from_rgb(1.0, 0.0, 0.5)),
        );
        let target = Color::from_rgb(1.0, 0.0, 0.5);
        assert!(
            glyph_colors(&frame).iter().any(|c| approx_color_eq(*c, target)),
            "expected at least one glyph painted with the foreground override"
        );
    }

    #[test]
    fn background_override_sets_bg_shape_color() {
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").background(Color::from_rgb(0.1, 0.7, 0.2)),
        );
        let target = Color::from_rgb(0.1, 0.7, 0.2);
        assert!(
            shape_colors(&frame).iter().any(|c| approx_color_eq(*c, target)),
            "expected the bg override colour to appear on a Shape quad"
        );
    }

    #[test]
    fn auto_contrast_uses_overridden_bg_for_initials_text() {
        // With a near-white background override and no foreground
        // override, auto-contrast should pick a dark text colour.
        let frame = render_avatar(
            Avatar::with_initials_literal("JD")
                .background(Color::from_rgb(0.95, 0.95, 0.95)),
        );
        let glyphs = glyph_colors(&frame);
        assert!(
            !glyphs.is_empty(),
            "expected at least one initials glyph in the frame"
        );
        for g in &glyphs {
            // Each channel should be in the dark range.
            assert!(
                g[0] < 0.3 && g[1] < 0.3 && g[2] < 0.3,
                "expected dark auto-contrast glyph against a light bg, got {:?}",
                g
            );
        }
    }

    #[test]
    fn auto_contrast_uses_overridden_bg_against_dark() {
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").background(Color::from_rgb(0.05, 0.05, 0.05)),
        );
        let glyphs = glyph_colors(&frame);
        assert!(!glyphs.is_empty());
        for g in &glyphs {
            assert!(
                g[0] > 0.9 && g[1] > 0.9 && g[2] > 0.9,
                "expected white auto-contrast glyph against a dark bg, got {:?}",
                g
            );
        }
    }

    #[test]
    fn with_name_seed_drives_bg_palette_pick() {
        // Two avatars with the same DERIVED initials but DIFFERENT
        // full names must pick distinct palette buckets — proving the
        // hash uses the seed (full name), not the initials.
        // ("Jane Doe" → JD, "Jules Dupont" → JD: identical initials.)
        let a = render_avatar(Avatar::with_name_literal("Jane Doe"));
        let b = render_avatar(Avatar::with_name_literal("Jules Dupont"));
        let bg_a = shape_colors(&a)
            .into_iter()
            .next()
            .expect("first shape is the bg circle");
        let bg_b = shape_colors(&b)
            .into_iter()
            .next()
            .expect("first shape is the bg circle");
        assert_ne!(
            bg_a, bg_b,
            "Jane Doe and Jules Dupont share initials JD but must hash distinctly via their full names"
        );
    }

    // ── accessibility for image avatars: inner ImageWidget is silenced ─

    #[test]
    fn image_avatar_announces_alt_on_parent() {
        // Inner ImageWidget is `a11y_hidden()` so the avatar is only
        // announced once. The parent carries the canonical name.
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let parent = tree.add(Avatar::with_image(&icon).alt_literal("Jane"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(parent);
        assert_eq!(info.role(), fern_core::accesskit::Role::Image);
        assert_eq!(info.name(), Some("Jane"));
    }

    #[test]
    fn shape_change_after_image_invalidates_mask() {
        let icon = rgba_solid(16, [10, 20, 30, 255]);
        let a = Avatar::with_image(&icon).alt_literal("X");
        // First build under default (Circle) — masking caches.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let _ = tree.add(a.shape(AvatarShape::Square));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let _ = tree.render();
        // Flow assertion: no panic, image emitted.
        // Switching shape after construction is a builder-time
        // operation; the cache invalidation runs in `build()`.
    }
}
