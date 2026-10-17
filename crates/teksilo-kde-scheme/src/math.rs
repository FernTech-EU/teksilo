// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The colour arithmetic KDE Plasma derives its palettes with.
//!
//! Every operation takes and returns [`KdeColor`]s, four 16-bit channels,
//! and follows the precision rules of section a.3 of Teksilo's
//! specification (`engineering/docs/kde-breeze-spec.md`) so that a result
//! reduced to 8 bits is the byte KDE itself writes:
//!
//! - a channel `q` is read as the single-precision quotient `q / 65535`,
//!   then widened to `f64` ([`read_channel`]);
//! - a real result `v` is stored by rounding it to single precision,
//!   forming the single-precision product `v x 65535`, and rounding that
//!   half up ([`store_channel`]);
//! - everything in between is `f64`, evaluated in the order the
//!   specification writes it, except where a step says single precision.
//!
//! The operations, in the specification's order (section d):
//!
//! | Operation | Function |
//! |---|---|
//! | HCY colour space (gamma 2.2, Rec. 709 weights) | [`Hcy::from_color`], [`Hcy::to_color`] |
//! | luma, and the luma contrast ratio | [`luma`], [`luma_ratio`], [`color_luma_ratio`] |
//! | lighten, darken, shade in HCY | [`lighten`], [`darken`], [`shade`] |
//! | premultiplied mix | [`mix`] |
//! | luma-preserving tint | [`tint`] |
//! | the five shade roles of a palette | [`shade_role`] |
//! | Qt's HSV lighter and darker | [`hsv_lighter`], [`hsv_darker`] |
//! | the 8-bit alpha blend | [`blend8`] |
//! | black or white on a background | [`black_or_white_on`] |
//! | an accent laid over a background | [`accent_over`] |
//! | the OKLab tint, and the sRGB curve under it | [`oklab_tint`], [`srgb_to_linear`], [`linear_to_srgb`] |
//!
//! The luma contrast ratio is KDE's own measure, built on a pure 2.2 power;
//! it is **not** WCAG contrast and must never be used to check
//! readability. [`lift_luma_until`] is Teksilo's helper for that: it keeps a
//! colour's HCY hue and chroma and moves its luma until the WCAG 2.x ratio
//! reaches a floor, measured with [`teksilo_tokens::contrast`].

use teksilo_tokens::contrast::composite_contrast;

use crate::color::KdeColor;

/// The red weight of the luma, ITU-R BT.709.
pub const LUMA_RED: f64 = 0.2126;
/// The green weight of the luma, ITU-R BT.709.
pub const LUMA_GREEN: f64 = 0.7152;
/// The blue weight of the luma, ITU-R BT.709.
pub const LUMA_BLUE: f64 = 0.0722;
/// The gamma exponent the HCY space linearises channels with.
pub const GAMMA: f64 = 2.2;

// ---------------------------------------------------------------------------
// Channel conventions (section a.3)
// ---------------------------------------------------------------------------

/// The real value of a stored channel: `q / 65535` in single precision,
/// widened to `f64`.
pub fn read_channel(q: u16) -> f64 {
    f64::from(f32::from(q) / 65535.0_f32)
}

/// Stores a real channel value in `0..=1` as a 16-bit channel.
///
/// The value is rounded to single precision, multiplied by 65535 in single
/// precision, and the product rounded half up. A value below 0 stores 0
/// and one above 1 stores 65535; a NaN stores 0. (Every operation of this
/// module clamps before it stores, so the last two cases are only reached
/// through a direct call.)
pub fn store_channel(v: f64) -> u16 {
    let product = (v as f32) * 65535.0_f32;
    let rounded = (f64::from(product) + 0.5).floor();
    if rounded.is_nan() || rounded <= 0.0 {
        0
    } else if rounded >= 65535.0 {
        u16::MAX
    } else {
        rounded as u16
    }
}

/// `N(x)` of the specification: `x` inside `(0, 1)`, 0 at or below 0, and 1
/// at or above 1 **or for a NaN**.
fn unit(x: f64) -> f64 {
    if x > 0.0 && x < 1.0 {
        x
    } else if x <= 0.0 {
        0.0
    } else {
        1.0
    }
}

/// `L(a, b, t) = a + (b - a) t`, in this form.
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// A channel read made linear: `N(x)^2.2`.
fn linearise(x: f64) -> f64 {
    unit(x).powf(GAMMA)
}

/// A linear value back to a channel: `N(x)^(1/2.2)`.
fn delinearise(x: f64) -> f64 {
    unit(x).powf(1.0 / GAMMA)
}

fn reads(c: KdeColor) -> [f64; 4] {
    [
        read_channel(c.r),
        read_channel(c.g),
        read_channel(c.b),
        read_channel(c.a),
    ]
}

// ---------------------------------------------------------------------------
// HCY and luma (sections d.2 to d.5)
// ---------------------------------------------------------------------------

/// A colour in KDE's HCY space: hue, chroma, luma and alpha, each a real
/// number.
///
/// The luma `y` is the Rec. 709 weighted sum of the gamma-2.2 linear
/// channels ([`luma`]). The hue `h` is in turns; [`Hcy::from_color`] does
/// **not** wrap it, so a colour whose red channel is the largest and whose
/// blue exceeds its green has a hue in `(-1/6, 0)`. [`Hcy::to_color`]
/// wraps any hue, and [`Hcy::wrapped_hue`] gives it in `[0, 1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hcy {
    /// Hue in turns, possibly negative (see the type documentation).
    pub h: f64,
    /// Chroma, 0 for a grey and 1 at the edge of the gamut.
    pub c: f64,
    /// Luma, 0 for black and 1 for white.
    pub y: f64,
    /// Alpha, the channel read of the colour's alpha.
    pub a: f64,
}

impl Hcy {
    /// Converts a stored colour to HCY.
    pub fn from_color(color: KdeColor) -> Hcy {
        let [rr, gr, br, a] = reads(color);
        let r = linearise(rr);
        let g = linearise(gr);
        let b = linearise(br);
        let y = r * LUMA_RED + g * LUMA_GREEN + b * LUMA_BLUE;
        let p = r.max(g).max(b);
        let n = r.min(g).min(b);
        let d = 6.0 * (p - n);
        let h = if n == p {
            0.0
        } else if r == p {
            (g - b) / d
        } else if g == p {
            (b - r) / d + 1.0 / 3.0
        } else {
            (r - g) / d + 2.0 / 3.0
        };
        let c = if r == g && g == b {
            0.0
        } else {
            ((y - n) / y).max((p - y) / (1.0 - y))
        };
        Hcy { h, c, y, a }
    }

    /// The hue in `[0, 1)`: [`Hcy::h`] plus 1 when it is negative.
    pub fn wrapped_hue(self) -> f64 {
        if self.h < 0.0 { self.h + 1.0 } else { self.h }
    }

    /// Converts back to a stored colour.
    ///
    /// The hue is wrapped into `[0, 1)` and the chroma and luma clamped to
    /// `0..=1`; each linear component is clamped, raised to `1/2.2` and
    /// stored, and so is the alpha. Converting an 8-bit colour to HCY and
    /// back gives the same colour.
    pub fn to_color(self) -> KdeColor {
        let h = wrap_hue(self.h);
        let c = unit(self.c);
        let y = unit(self.y);
        let s = 6.0 * h;
        let (sector, t, m) = if s < 1.0 {
            (0, s, LUMA_RED + LUMA_GREEN * s)
        } else if s < 2.0 {
            let t = 2.0 - s;
            (1, t, LUMA_GREEN + LUMA_RED * t)
        } else if s < 3.0 {
            let t = s - 2.0;
            (2, t, LUMA_GREEN + LUMA_BLUE * t)
        } else if s < 4.0 {
            let t = 4.0 - s;
            (3, t, LUMA_BLUE + LUMA_GREEN * t)
        } else if s < 5.0 {
            let t = s - 4.0;
            (4, t, LUMA_BLUE + LUMA_RED * t)
        } else {
            let t = 6.0 - s;
            (5, t, LUMA_RED + LUMA_BLUE * t)
        };
        let (top, middle, bottom) = if m >= y {
            (
                y + y * c * (1.0 - m) / m,
                y + y * c * (t - m) / m,
                y - y * c,
            )
        } else {
            (
                y + (1.0 - y) * c,
                y + (1.0 - y) * c * (t - m) / (1.0 - m),
                y - (1.0 - y) * c * m / (1.0 - m),
            )
        };
        let (r, g, b) = match sector {
            0 => (top, middle, bottom),
            1 => (middle, top, bottom),
            2 => (bottom, top, middle),
            3 => (bottom, middle, top),
            4 => (middle, bottom, top),
            _ => (top, bottom, middle),
        };
        KdeColor::rgba16(
            store_channel(delinearise(r)),
            store_channel(delinearise(g)),
            store_channel(delinearise(b)),
            store_channel(self.a),
        )
    }
}

/// The remainder of `h` by 1 with `h`'s sign, plus 1 when negative; a zero
/// or NaN remainder gives 0.
fn wrap_hue(h: f64) -> f64 {
    let r = h % 1.0;
    if r.is_nan() || r == 0.0 {
        0.0
    } else if r < 0.0 {
        r + 1.0
    } else {
        r
    }
}

/// The luma of a colour, in `0..=1`: the Rec. 709 weighted sum of its
/// channels raised to the power 2.2. Alpha is ignored.
///
/// This is not WCAG relative luminance, which uses the sRGB transfer curve.
pub fn luma(color: KdeColor) -> f64 {
    let [r, g, b, _] = reads(color);
    linearise(r) * LUMA_RED + linearise(g) * LUMA_GREEN + linearise(b) * LUMA_BLUE
}

/// KDE's contrast ratio between two lumas: `(Y1 + 0.05) / (Y2 + 0.05)` with
/// the larger luma on top, so the result is at least 1.
///
/// It has the shape of WCAG's ratio but is computed on [`luma`], so its
/// values differ from WCAG contrast. Use it only to reproduce KDE's
/// decisions, never to judge readability.
pub fn luma_ratio(y1: f64, y2: f64) -> f64 {
    if y1 > y2 {
        (y1 + 0.05) / (y2 + 0.05)
    } else {
        (y2 + 0.05) / (y1 + 0.05)
    }
}

/// [`luma_ratio`] of the lumas of two colours.
pub fn color_luma_ratio(a: KdeColor, b: KdeColor) -> f64 {
    luma_ratio(luma(a), luma(b))
}

// ---------------------------------------------------------------------------
// Luma operations, mix and tint (sections d.6 to d.9)
// ---------------------------------------------------------------------------

/// Moves a colour's luma toward white, keeping its hue and alpha.
///
/// `amount` 1 gives white, 0.5 halves the distance to white and 0 keeps
/// the luma: `y' = 1 - N((1 - y)(1 - amount))`. The chroma becomes
/// `1 - N((1 - c) chroma_factor)`, so the usual factor 1 keeps it. KDE's
/// defaults are `amount` 0.5 and `chroma_factor` 1.
///
/// The colour is converted through HCY even when nothing changes, so the
/// result may differ from the input by one 16-bit step.
pub fn lighten(color: KdeColor, amount: f64, chroma_factor: f64) -> KdeColor {
    let mut hcy = Hcy::from_color(color);
    hcy.y = 1.0 - unit((1.0 - hcy.y) * (1.0 - amount));
    hcy.c = 1.0 - unit((1.0 - hcy.c) * chroma_factor);
    hcy.to_color()
}

/// Moves a colour's luma toward black, keeping its hue and alpha.
///
/// `y' = N(y (1 - amount))` and `c' = N(c chroma_factor)`; KDE's defaults
/// are `amount` 0.5 and `chroma_factor` 1. An `amount` of 0 with a
/// `chroma_factor` below 1 desaturates.
pub fn darken(color: KdeColor, amount: f64, chroma_factor: f64) -> KdeColor {
    let mut hcy = Hcy::from_color(color);
    hcy.y = unit(hcy.y * (1.0 - amount));
    hcy.c = unit(hcy.c * chroma_factor);
    hcy.to_color()
}

/// Adds `luma_amount` to a colour's luma and `chroma_amount` to its
/// chroma, each clamped to `0..=1`, keeping its hue and alpha. A negative
/// amount darkens or desaturates.
pub fn shade(color: KdeColor, luma_amount: f64, chroma_amount: f64) -> KdeColor {
    let mut hcy = Hcy::from_color(color);
    hcy.y = unit(hcy.y + luma_amount);
    hcy.c = unit(hcy.c + chroma_amount);
    hcy.to_color()
}

/// Interpolates from `a` (at `t` 0) to `b` (at `t` 1) with premultiplied
/// alpha.
///
/// At or below 0 the result is `a` exactly, at or above 1 it is `b`
/// exactly, and a NaN `t` gives `a`. Otherwise the alpha is interpolated
/// and each channel is interpolated premultiplied (the products in single
/// precision) and divided back by that alpha; a result with no alpha left
/// is transparent black.
pub fn mix(a: KdeColor, b: KdeColor, t: f64) -> KdeColor {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    if t.is_nan() {
        return a;
    }
    let ra = reads(a);
    let rb = reads(b);
    let alpha = lerp(ra[3], rb[3], t);
    if alpha <= 0.0 {
        return KdeColor::TRANSPARENT;
    }
    let channel = |i: usize| {
        let pa = f64::from((ra[i] as f32) * (ra[3] as f32));
        let pb = f64::from((rb[i] as f32) * (rb[3] as f32));
        store_channel(lerp(pa, pb, t).clamp(0.0, 1.0) / alpha)
    };
    KdeColor::rgba16(channel(0), channel(1), channel(2), store_channel(alpha))
}

/// Tints `base` toward `color` by `amount` while keeping `base`'s luma
/// within a bounded contrast of where it was.
///
/// At or below 0 the result is `base`, at or above 1 it is `color`, and a
/// NaN amount gives `base`. Otherwise the mix factor is found by twelve
/// bisection steps against a luma-ratio budget that grows with the cube of
/// `amount`, and the result is that mix with its luma pulled back toward
/// `base`'s. KDE's default amount is 0.3.
pub fn tint(base: KdeColor, color: KdeColor, amount: f64) -> KdeColor {
    if amount <= 0.0 {
        return base;
    }
    if amount >= 1.0 {
        return color;
    }
    if amount.is_nan() {
        return base;
    }
    let base_luma = luma(base);
    let initial_ratio = luma_ratio(base_luma, luma(color));
    let budget = 1.0 + (initial_ratio + 1.0) * amount * amount * amount;
    let mut low = 0.0_f64;
    let mut high = 1.0_f64;
    let mut s = 0.5;
    for _ in 0..12 {
        s = (low + high) / 2.0;
        let mixed = mix(base, color, s.powf(0.3));
        let mixed_luma = lerp(base_luma, luma(mixed), s);
        if luma_ratio(base_luma, mixed_luma) > budget {
            high = s;
        } else {
            low = s;
        }
    }
    let mut hcy = Hcy::from_color(mix(base, color, s.powf(0.3)));
    hcy.y = lerp(base_luma, hcy.y, s);
    hcy.to_color()
}

// ---------------------------------------------------------------------------
// Shade roles (section d.10)
// ---------------------------------------------------------------------------

/// The five 3D shades a palette derives from its window background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadeRole {
    /// The lightest shade, a bevel's lit edge.
    Light,
    /// Between the background and [`ShadeRole::Light`].
    Midlight,
    /// Between the background and [`ShadeRole::Dark`].
    Mid,
    /// A bevel's shadowed edge.
    Dark,
    /// The darkest shade, a drop shadow.
    Shadow,
}

impl ShadeRole {
    /// The five roles, lightest first.
    pub const ALL: [ShadeRole; 5] = [
        ShadeRole::Light,
        ShadeRole::Midlight,
        ShadeRole::Mid,
        ShadeRole::Dark,
        ShadeRole::Shadow,
    ];
}

/// The shade of `color` for `role`, at contrast value `contrast`.
///
/// `contrast` is the scheme's `[KDE] contrast` divided by 10 (0.4 for
/// Breeze), clamped to `-1..=1` (a NaN counts as 1). Very dark colours
/// (luma below 0.006) shade upward and very light ones (above 0.93)
/// downward; anything between gets a lighter Light and Midlight and a
/// darker Mid, Dark and Shadow, scaled by its luma. `chroma_amount` is
/// added to the chroma of every shade (KDE's default is 0).
pub fn shade_role(color: KdeColor, role: ShadeRole, contrast: f64, chroma_amount: f64) -> KdeColor {
    let k = if contrast >= 1.0 || contrast.is_nan() {
        1.0
    } else if contrast <= -1.0 {
        -1.0
    } else {
        contrast
    };
    let y = luma(color);
    let yi = 1.0 - y;
    let shaded = |amount: f64| shade(color, amount, chroma_amount);
    if y < 0.006 {
        match role {
            ShadeRole::Light => shaded(0.05 + 0.95 * k),
            ShadeRole::Mid => shaded(0.01 + 0.20 * k),
            ShadeRole::Dark => shaded(0.02 + 0.40 * k),
            ShadeRole::Midlight | ShadeRole::Shadow => shaded(0.03 + 0.60 * k),
        }
    } else if y > 0.93 {
        match role {
            ShadeRole::Midlight => shaded(-0.02 - 0.20 * k),
            ShadeRole::Dark => shaded(-0.06 - 0.60 * k),
            ShadeRole::Shadow => shaded(-0.10 - 0.90 * k),
            ShadeRole::Light | ShadeRole::Mid => shaded(-0.04 - 0.40 * k),
        }
    } else {
        let light_amount = (0.05 + 0.55 * y) * (0.25 + 0.75 * k);
        let dark_amount = (-y) * (0.55 + 0.35 * k);
        match role {
            ShadeRole::Light => shaded(light_amount),
            ShadeRole::Midlight => shaded((0.15 + 0.35 * yi) * light_amount),
            ShadeRole::Mid => shaded((0.35 + 0.15 * y) * dark_amount),
            ShadeRole::Dark => shaded(dark_amount),
            ShadeRole::Shadow => darken(shaded(dark_amount), 0.5 + 0.3 * y, 1.0),
        }
    }
}

// ---------------------------------------------------------------------------
// HSV lighter and darker (section d.11)
// ---------------------------------------------------------------------------

/// A colour in Qt's integer HSV form: hue in hundredths of a degree
/// (`None` for a grey), saturation and value in `0..=65535`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hsv16 {
    hue: Option<u32>,
    saturation: u32,
    value: u32,
}

/// `R(x) = floor(x + 1/2)` of a non-negative single-precision value.
fn round_half_up(x: f32) -> u32 {
    (f64::from(x) + 0.5).floor() as u32
}

fn to_hsv16(color: KdeColor) -> Hsv16 {
    let (r, g, b) = (u32::from(color.r), u32::from(color.g), u32::from(color.b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == min {
        return Hsv16 {
            hue: None,
            saturation: 0,
            value: max,
        };
    }
    let k = (max - min) as f32;
    let saturation = round_half_up((k / max as f32) * 65535.0_f32);
    let diff = |a: u32, b: u32| (i64::from(a) - i64::from(b)) as f32;
    let position = if max == r {
        let p = diff(g, b) / k;
        if p < 0.0 { p + 6.0_f32 } else { p }
    } else if max == g {
        2.0_f32 + diff(b, r) / k
    } else {
        4.0_f32 + diff(r, g) / k
    };
    Hsv16 {
        hue: Some(round_half_up(position * 6000.0_f32)),
        saturation,
        value: max,
    }
}

fn from_hsv16(hsv: Hsv16, alpha: u16) -> KdeColor {
    let v16 = hsv.value as u16;
    let Some(hue) = hsv.hue.filter(|_| hsv.saturation != 0) else {
        return KdeColor::rgba16(v16, v16, v16, alpha);
    };
    let h = if hue == 36000 {
        0.0_f32
    } else {
        hue as f32 / 6000.0_f32
    };
    let s = hsv.saturation as f32 / 65535.0_f32;
    let v = hsv.value as f32 / 65535.0_f32;
    let i = h.trunc();
    let f = h - i;
    let p = v * (1.0_f32 - s);
    let q = v * (1.0_f32 - s * f);
    let t = v * (1.0_f32 - s * (1.0_f32 - f));
    let (r, g, b) = match i as u32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let store = |x: f32| round_half_up(x * 65535.0_f32).min(65535) as u16;
    KdeColor::rgba16(store(r), store(g), store(b), alpha)
}

/// Qt's documented `lighter(factor)`: the colour with its HSV value
/// multiplied by `factor / 100`.
///
/// A factor of 0 or less returns the colour unchanged, and a factor below
/// 100 is [`hsv_darker`] by `10000 / factor`. When the value would pass the
/// top of the range, the excess is taken off the saturation instead, so a
/// strong factor moves toward white. The colour goes through Qt's 16-bit
/// HSV form even at factor 100, and keeps its alpha.
pub fn hsv_lighter(color: KdeColor, factor: i32) -> KdeColor {
    if factor <= 0 {
        return color;
    }
    if factor < 100 {
        return hsv_darker(color, 10000 / factor);
    }
    let mut hsv = to_hsv16(color);
    let value = i64::from(factor) * i64::from(hsv.value) / 100;
    if value > 65535 {
        let excess = value - 65535;
        hsv.saturation = (i64::from(hsv.saturation) - excess).max(0) as u32;
        hsv.value = 65535;
    } else {
        hsv.value = value as u32;
    }
    from_hsv16(hsv, color.a)
}

/// Qt's documented `darker(factor)`: the colour with its HSV value divided
/// by `factor / 100`.
///
/// A factor of 0 or less returns the colour unchanged, and a factor below
/// 100 is [`hsv_lighter`] by `10000 / factor`. The saturation and alpha are
/// kept.
pub fn hsv_darker(color: KdeColor, factor: i32) -> KdeColor {
    if factor <= 0 {
        return color;
    }
    if factor < 100 {
        return hsv_lighter(color, 10000 / factor);
    }
    let mut hsv = to_hsv16(color);
    hsv.value = (i64::from(hsv.value) * 100 / i64::from(factor)) as u32;
    from_hsv16(hsv, color.a)
}

// ---------------------------------------------------------------------------
// 8-bit blending (sections d.12 to d.14)
// ---------------------------------------------------------------------------

/// `foreground` composited over `background` the way KDE's accent code
/// does it, in 8-bit channels.
///
/// A fully transparent foreground returns `background` unchanged. Over an
/// opaque background each channel is `trunc(af f + (1 - af) g)`, with `af`
/// the foreground's alpha, `f` and `g` the 8-bit channels, and the first
/// product in single precision; the result is opaque. Over a translucent
/// background the colour channels are weighted by the background's alpha
/// as well, without un-premultiplying, and the alpha is the integer part
/// of the summed coverages (so 0, or 1 out of 255): KDE writes that nearly
/// transparent value and this function reproduces it.
pub fn blend8(foreground: KdeColor, background: KdeColor) -> KdeColor {
    let af = f32::from(foreground.a) / 65535.0_f32;
    if af == 0.0 {
        return background;
    }
    let inverse = 1.0 - f64::from(af);
    let [fr, fg, fb, _] = foreground.to_rgba8();
    let [gr, gg, gb, _] = background.to_rgba8();
    let background_alpha = read_channel(background.a);
    let weight = if background_alpha == 1.0 {
        inverse
    } else {
        background_alpha * inverse
    };
    let channel = |f: u8, g: u8| {
        let painted = f64::from(af * f32::from(f));
        (painted + weight * f64::from(g)).trunc().clamp(0.0, 255.0) as u8
    };
    let alpha = if background_alpha == 1.0 {
        255
    } else {
        (f64::from(af) + weight).trunc().clamp(0.0, 255.0) as u8
    };
    KdeColor::rgba8(channel(fr, gr), channel(fg, gg), channel(fb, gb), alpha)
}

/// Black or white text for `background`, as KDE picks it for an accent
/// colour.
///
/// Black when white would reach less than 4.5 in [`luma_ratio`] (KDE's
/// measure, not WCAG's: black exactly when the luma is above about
/// 0.183), white otherwise. An active colour is that colour, opaque; an
/// inactive one is it at alpha 0.6 laid over `background` with
/// [`blend8`].
pub fn black_or_white_on(background: KdeColor, active: bool) -> KdeColor {
    let chosen = if color_luma_ratio(background, KdeColor::WHITE) < 4.5 {
        KdeColor::BLACK
    } else {
        KdeColor::WHITE
    };
    let alpha = if active { 1.0 } else { 0.6 };
    blend8(with_alpha(chosen, alpha), background)
}

/// An accent laid over a background at alpha 0.7 with [`blend8`]: the
/// selection fill KDE derives from an accent. Only the accent's 8-bit
/// channels are used; its own alpha is replaced.
pub fn accent_over(accent: KdeColor, background: KdeColor) -> KdeColor {
    let [r, g, b, _] = accent.to_rgba8();
    blend8(with_alpha(KdeColor::rgb8(r, g, b), 0.7), background)
}

/// `color` with its alpha set to the fraction `alpha`, stored like any
/// channel.
fn with_alpha(color: KdeColor, alpha: f64) -> KdeColor {
    KdeColor {
        a: store_channel(alpha),
        ..color
    }
}

// ---------------------------------------------------------------------------
// sRGB curve and OKLab tint (section d.15)
// ---------------------------------------------------------------------------

/// A colour held as floating-point channels, the way Qt keeps the result
/// of a colour-space conversion.
///
/// A result whose three colour channels all lie in `0..=1` is stored in 16
/// bits like any colour. One with a channel outside that range is held in
/// half precision instead: each channel, alpha included, is rounded to the
/// nearest IEEE 754 binary16 value (ties to even), so it can carry the
/// out-of-gamut value into the next step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatColor {
    /// Every colour channel in range: 16-bit storage.
    Rgb16(KdeColor),
    /// Some colour channel out of range: red, green, blue and alpha, each
    /// an exact binary16 value written as an `f32`.
    Half([f32; 4]),
}

impl FloatColor {
    /// Stores single-precision channels by the rule of the type
    /// documentation. The range test is made on these `f32` values.
    pub fn from_f32(rgb: [f32; 3], alpha: f32) -> FloatColor {
        if rgb.iter().all(|c| (0.0..=1.0).contains(c)) {
            FloatColor::Rgb16(KdeColor::rgba16(
                store_channel(f64::from(rgb[0])),
                store_channel(f64::from(rgb[1])),
                store_channel(f64::from(rgb[2])),
                store_channel(f64::from(alpha)),
            ))
        } else {
            FloatColor::Half([
                round_to_half(rgb[0]),
                round_to_half(rgb[1]),
                round_to_half(rgb[2]),
                round_to_half(alpha),
            ])
        }
    }

    /// The channel reads: `q / 65535` in single precision for a 16-bit
    /// colour, the binary16 values for a half-precision one.
    pub fn channels(self) -> [f32; 4] {
        match self {
            FloatColor::Rgb16(c) => [c.r, c.g, c.b, c.a].map(|q| f32::from(q) / 65535.0_f32),
            FloatColor::Half(channels) => channels,
        }
    }

    /// The colour in 16-bit channels: a half-precision colour has each
    /// channel clamped to `0..=1` and stored.
    pub fn to_rgb16(self) -> KdeColor {
        match self {
            FloatColor::Rgb16(c) => c,
            FloatColor::Half([r, g, b, a]) => {
                let store = |x: f32| store_channel(f64::from(x.clamp(0.0, 1.0)));
                KdeColor::rgba16(store(r), store(g), store(b), store(a))
            }
        }
    }
}

/// Rounds a single-precision value to the nearest binary16 value, ties to
/// even, overflowing to infinity past the largest finite one.
fn round_to_half(x: f32) -> f32 {
    if x.is_nan() || x == 0.0 || x.is_infinite() {
        return x;
    }
    let magnitude = f64::from(x.abs());
    // 65520 is halfway between 65504, the largest finite binary16 value,
    // and 65536; the tie goes to the even neighbour, which is infinity.
    let rounded = if magnitude >= 65520.0 {
        f64::INFINITY
    } else {
        // Below 2^-14 the binary16 values are the subnormals, multiples of
        // 2^-24; above, ten fraction bits under the leading one.
        let exponent = ((magnitude.to_bits() >> 52) & 0x7ff) as i32 - 1023;
        let quantum = 2.0_f64.powi(exponent.max(-14) - 10);
        (magnitude / quantum).round_ties_even() * quantum
    };
    (rounded as f32).copysign(x)
}

/// The constants of Qt's sRGB transfer curve, each computed in single
/// precision exactly as section d.15 writes it.
struct SrgbCurve {
    a: f32,
    b: f32,
    k: f32,
    d: f32,
    g: f32,
    inv_d: f32,
    inv_k: f32,
    inv_a: f32,
    inv_e: f32,
    inv_g: f32,
}

/// A single-precision power, correctly rounded: computed in `f64` and
/// rounded once.
fn powf32(x: f32, y: f32) -> f32 {
    f64::from(x).powf(f64::from(y)) as f32
}

impl SrgbCurve {
    fn new() -> SrgbCurve {
        let a = 1.0_f32 / 1.055_f32;
        let b = 0.055_f32 / 1.055_f32;
        let k = 1.0_f32 / 12.92_f32;
        let d = 0.04045_f32;
        let g = 2.4_f32;
        SrgbCurve {
            a,
            b,
            k,
            d,
            g,
            inv_d: k * d,
            inv_k: 1.0_f32 / k,
            inv_a: powf32(1.0_f32 / a, g),
            inv_e: -b / a,
            inv_g: 1.0_f32 / g,
        }
    }

    /// The sRGB curve, channel to linear light, sign kept.
    fn forward(&self, x: f32) -> f32 {
        let u = x.abs();
        let t = if u < self.d {
            self.k * u
        } else {
            powf32(self.a * u + self.b, self.g)
        };
        t.copysign(x)
    }

    /// The inverse curve, linear light to channel, sign kept.
    fn inverse(&self, y: f32) -> f32 {
        let u = y.abs();
        let t = if u < self.inv_d {
            self.inv_k * u
        } else {
            powf32(self.inv_a * u, self.inv_g) + self.inv_e
        };
        t.copysign(y)
    }
}

/// The matrix Qt applies between its sRGB and linear sRGB spaces: the
/// identity up to single-precision rounding. Rows are red, green, blue;
/// the values are binary32 numbers given by their bit patterns.
const SRGB_MATRIX: [[u32; 3]; 3] = [
    // 0x1.fffffep-1, 0x1.4p-25, -0x1p-25
    [0x3f7f_ffff, 0x3320_0000, 0xb300_0000],
    // 0x1.928p-26, 1, 0
    [0x32c9_4000, 0x3f80_0000, 0x0000_0000],
    // 0, 0, 0x1.fffffcp-1
    [0x0000_0000, 0x0000_0000, 0x3f7f_fffe],
];

fn apply_srgb_matrix([r, g, b]: [f32; 3]) -> [f32; 3] {
    SRGB_MATRIX.map(|[m1, m2, m3]| {
        (r * f32::from_bits(m1) + g * f32::from_bits(m2)) + b * f32::from_bits(m3)
    })
}

/// Converts a colour from sRGB to linear sRGB as Qt's colour-space
/// transform does: the sRGB curve on each channel read, then the
/// near-identity matrix, all in single precision, stored as a
/// [`FloatColor`]. Alpha is carried through.
pub fn srgb_to_linear(color: FloatColor) -> FloatColor {
    let curve = SrgbCurve::new();
    let [r, g, b, a] = color.channels();
    let linear = apply_srgb_matrix([curve.forward(r), curve.forward(g), curve.forward(b)]);
    FloatColor::from_f32(linear, a)
}

/// Converts a colour from linear sRGB to sRGB as Qt's colour-space
/// transform does: the near-identity matrix, then the inverse sRGB curve
/// on each channel, all in single precision, stored as a [`FloatColor`].
/// Alpha is carried through.
pub fn linear_to_srgb(color: FloatColor) -> FloatColor {
    let curve = SrgbCurve::new();
    let [r, g, b, a] = color.channels();
    let [lr, lg, lb] = apply_srgb_matrix([r, g, b]);
    FloatColor::from_f32([curve.inverse(lr), curve.inverse(lg), curve.inverse(lb)], a)
}

/// Linear sRGB to OKLab with Ottosson's published matrices, in `f64`.
fn linear_to_oklab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let (l, m, s) = (l.powf(1.0 / 3.0), m.powf(1.0 / 3.0), s.powf(1.0 / 3.0));
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// OKLab back to linear sRGB with Ottosson's published matrices, in `f64`.
fn oklab_to_linear([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let l = lightness + 0.3963377774 * a + 0.2158037573 * b;
    let m = lightness - 0.1055613458 * a - 0.0638541728 * b;
    let s = lightness - 0.0894841775 * a - 1.2914855480 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

/// `x (1 - f) + y f` with the mixed precision of section d.15 step 3: the
/// inputs rounded to single precision, `y f` in single precision, the rest
/// in double, the result rounded to single precision.
fn tint_lerp(x: f64, y: f64, f: f64) -> f32 {
    let (x, y, f) = (x as f32, y as f32, f as f32);
    let kept = f64::from(x) * (1.0 - f64::from(f));
    (kept + f64::from(y * f)) as f32
}

/// Tints `base` toward `color` in OKLab by `factor`, keeping `base`'s
/// perceptual lightness: the accent tint Plasma applies to every colour of
/// a scheme that sets a tint factor.
///
/// Both colours are taken to linear sRGB ([`srgb_to_linear`]) and to
/// OKLab; the result has `base`'s lightness and its `a` and `b` moved
/// `factor` of the way toward `color`'s. It is taken back to linear sRGB,
/// stored, reduced to 16 bits and converted to sRGB ([`linear_to_srgb`]);
/// a result outside the gamut is clamped. The result is opaque.
pub fn oklab_tint(base: KdeColor, color: KdeColor, factor: f64) -> KdeColor {
    let oklab_of = |c: KdeColor| {
        let [r, g, b, _] = srgb_to_linear(FloatColor::Rgb16(c)).channels();
        linear_to_oklab([f64::from(r), f64::from(g), f64::from(b)])
    };
    let [lightness, base_a, base_b] = oklab_of(base);
    let [_, color_a, color_b] = oklab_of(color);
    let a = f64::from(tint_lerp(base_a, color_a, factor));
    let b = f64::from(tint_lerp(base_b, color_b, factor));
    let linear = oklab_to_linear([lightness, a, b]).map(|c| c as f32);
    let stored = FloatColor::from_f32(linear, 1.0).to_rgb16();
    let srgb = linear_to_srgb(FloatColor::Rgb16(KdeColor {
        a: u16::MAX,
        ..stored
    }));
    KdeColor {
        a: u16::MAX,
        ..srgb.to_rgb16()
    }
}

// ---------------------------------------------------------------------------
// Teksilo's WCAG lift
// ---------------------------------------------------------------------------

/// The number of equal luma steps [`lift_luma_until`] tries between the
/// colour and black or white. A step of 1/256 of the way moves an 8-bit
/// channel by about one level, so the lift overshoots its floor by very
/// little.
const LUMA_LIFT_STEPS: u32 = 256;

/// `color` with its HCY luma moved toward more contrast until its WCAG 2.x
/// contrast ratio reaches `floor` against every colour in `backdrops`.
///
/// This is Teksilo's helper, not KDE's: KDE never lifts a colour for
/// readability. The hue, chroma and alpha are kept, so a lifted accent
/// stays recognisably the same hue; only its luma moves, toward black or
/// toward white, whichever reaches the higher minimum contrast over
/// `backdrops`. Contrast is measured with
/// [`teksilo_tokens::contrast::composite_contrast`], so a translucent
/// `color` is measured as painted, and each backdrop is taken as opaque.
///
/// Returns `color` unchanged when it already passes, or when `backdrops` is
/// empty. Otherwise returns the first luma step that passes, or, when no
/// step does (a floor above what black or white can reach, a very
/// translucent `color`, or backdrops on both sides of the middle), the step
/// with the highest minimum contrast.
pub fn lift_luma_until(color: KdeColor, backdrops: &[KdeColor], floor: f32) -> KdeColor {
    let worst = |c: KdeColor| -> f32 {
        let fg = c.to_color();
        backdrops
            .iter()
            .map(|bg| composite_contrast(fg, bg.to_color()))
            .fold(f32::INFINITY, f32::min)
    };
    if backdrops.is_empty() || worst(color) >= floor {
        return color;
    }
    let hcy = Hcy::from_color(color);
    let at_luma = |y: f64| Hcy { y, ..hcy }.to_color();
    let target = if worst(at_luma(0.0)) >= worst(at_luma(1.0)) {
        0.0
    } else {
        1.0
    };
    let mut best = (worst(color), color);
    for step in 1..=LUMA_LIFT_STEPS {
        let candidate = at_luma(lerp(
            hcy.y,
            target,
            f64::from(step) / f64::from(LUMA_LIFT_STEPS),
        ));
        let score = worst(candidate);
        if score >= floor {
            return candidate;
        }
        if score > best.0 {
            best = (score, candidate);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_bit_patterns_are_the_published_values() {
        // 0x1.fffffep-1, 0x1.4p-25, -0x1p-25 / 0x1.928p-26, 1, 0 /
        // 0, 0, 0x1.fffffcp-1, written as exact powers of two.
        let p = |e: i32| 2.0_f64.powi(e);
        let expected = [
            [1.0 - p(-24), 1.25 * p(-25), -p(-25)],
            [(1.0 + 2344.0 / 4096.0) * p(-26), 1.0, 0.0],
            [0.0, 0.0, 1.0 - p(-23)],
        ];
        for (row, expected) in SRGB_MATRIX.iter().zip(expected) {
            for (&bits, value) in row.iter().zip(expected) {
                assert_eq!(f64::from(f32::from_bits(bits)), value);
            }
        }
    }

    #[test]
    fn half_precision_rounding_keeps_ties_even_and_subnormals() {
        let p = |e: i32| 2.0_f32.powi(e);
        assert_eq!(round_to_half(1.0), 1.0);
        // Ten fraction bits: 1.05 becomes 1075 / 1024.
        assert_eq!(round_to_half(1.05), 1075.0 * p(-10));
        // Below 2^-14 the step is the subnormal 2^-24.
        assert_eq!(round_to_half(-1e-7), -2.0 * p(-24));
        // Exactly half the smallest subnormal ties to zero, keeping the sign.
        assert_eq!(round_to_half(-p(-25)).to_bits(), (-0.0_f32).to_bits());
        // A tie between two normal values goes to the even one.
        assert_eq!(round_to_half(1.0 + p(-11)), 1.0);
        assert_eq!(round_to_half(1.0 + 3.0 * p(-11)), 1.0 + p(-9));
        assert_eq!(round_to_half(65504.0), 65504.0);
        assert_eq!(round_to_half(65520.0), f32::INFINITY);
    }

    #[test]
    fn channel_storage_round_trips_every_16_bit_value() {
        for q in 0..=u16::MAX {
            assert_eq!(store_channel(read_channel(q)), q);
        }
    }
}
