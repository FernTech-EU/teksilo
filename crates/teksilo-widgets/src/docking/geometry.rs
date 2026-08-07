// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pure region-geometry engine for [`DockingLayout`](super::DockingLayout).
//!
//! A docking layout is a *border layout with configurable corners* — exactly
//! Qt's `QMainWindow` corner model, which a nested-`Splitter` tree cannot
//! express (in any splitter nesting the corners always belong to the outer
//! axis). So the five region rectangles are computed directly here, honouring
//! per-corner ownership, and the [`DockingLayout`](super::DockingLayout)
//! widget places its children from the result.
//!
//! Each side contributes three sub-rectangles: an always-visible **rail** strip
//! (the activity bar), a resizable/collapsible **content** rect, and a
//! **handle** (resize gutter) between the content and the centre. For the
//! **leading / trailing** columns the rail hugs the outer thickness edge with
//! the content inboard. For the **top / bottom** bands the (always vertical)
//! rail is instead a **column on the leading cross-edge** (left in LTR, right
//! in RTL) with the content inboard to its side — so it does not add to the
//! band depth. A hidden **leading / trailing** side keeps its rail (the reopen
//! affordance) but drops its content and handle; a hidden **top / bottom** band
//! collapses **completely** (rail included — a vertical rail can't stand alone
//! in a zero-depth band), so the app reveals it again via an external button.
//! Everything is clamped non-negative, so no container size — down to `0×0` or
//! smaller-than-the-sum-of-minimums — can produce a negative or overlapping
//! rectangle.

use serde::{Deserialize, Serialize};
use teksilo_canvas::Rect;

/// Below this the content/gutter is treated as fully collapsed.
const EPS: f32 = 0.01;

/// One of the four dockable sides. `Leading`/`Trailing` are
/// writing-direction-relative (mirrored under RTL by the caller); `Top`/
/// `Bottom` never mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockSide {
    /// Left in LTR, right in RTL.
    Leading,
    /// Right in LTR, left in RTL.
    Trailing,
    Top,
    Bottom,
}

impl DockSide {
    /// All four sides, in a stable order.
    pub const ALL: [DockSide; 4] = [
        DockSide::Leading,
        DockSide::Trailing,
        DockSide::Top,
        DockSide::Bottom,
    ];

    /// True for the vertical columns (leading / trailing), whose long axis
    /// is vertical — they stack their dock content top-to-bottom.
    pub fn is_horizontal_axis(self) -> bool {
        matches!(self, DockSide::Leading | DockSide::Trailing)
    }
}

/// One of the four corners of the container. Each corner is owned by exactly
/// one of its two adjacent sides (Qt `setCorner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockCorner {
    TopLeading,
    TopTrailing,
    BottomLeading,
    BottomTrailing,
}

impl DockCorner {
    /// All four corners.
    pub const ALL: [DockCorner; 4] = [
        DockCorner::TopLeading,
        DockCorner::TopTrailing,
        DockCorner::BottomLeading,
        DockCorner::BottomTrailing,
    ];

    /// The two sides adjacent to this corner: `(horizontal side, vertical
    /// side)` — i.e. `(Leading|Trailing, Top|Bottom)`.
    pub fn adjacent_sides(self) -> (DockSide, DockSide) {
        match self {
            DockCorner::TopLeading => (DockSide::Leading, DockSide::Top),
            DockCorner::TopTrailing => (DockSide::Trailing, DockSide::Top),
            DockCorner::BottomLeading => (DockSide::Leading, DockSide::Bottom),
            DockCorner::BottomTrailing => (DockSide::Trailing, DockSide::Bottom),
        }
    }

    /// Returns the *other* adjacent side (given one of the two).
    fn other(self, side: DockSide) -> DockSide {
        let (h, v) = self.adjacent_sides();
        if side == h { v } else { h }
    }
}

/// Which side owns each corner. Default = the classic IDE shell where the
/// top and bottom bars span the full width and the leading / trailing columns
/// occupy only the middle band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CornerOwners {
    pub top_leading: DockSide,
    pub top_trailing: DockSide,
    pub bottom_leading: DockSide,
    pub bottom_trailing: DockSide,
}

impl Default for CornerOwners {
    fn default() -> Self {
        Self {
            top_leading: DockSide::Top,
            top_trailing: DockSide::Top,
            bottom_leading: DockSide::Bottom,
            bottom_trailing: DockSide::Bottom,
        }
    }
}

impl CornerOwners {
    pub fn owner(&self, corner: DockCorner) -> DockSide {
        match corner {
            DockCorner::TopLeading => self.top_leading,
            DockCorner::TopTrailing => self.top_trailing,
            DockCorner::BottomLeading => self.bottom_leading,
            DockCorner::BottomTrailing => self.bottom_trailing,
        }
    }

    pub fn set(&mut self, corner: DockCorner, owner: DockSide) {
        match corner {
            DockCorner::TopLeading => self.top_leading = owner,
            DockCorner::TopTrailing => self.top_trailing = owner,
            DockCorner::BottomLeading => self.bottom_leading = owner,
            DockCorner::BottomTrailing => self.bottom_trailing = owner,
        }
    }
}

/// Per-side geometry inputs (all logical pixels), in LTR space.
#[derive(Debug, Clone, Copy)]
pub struct SideLayout {
    /// Stored content size along the side's thickness axis (width for
    /// leading/trailing, height for top/bottom).
    pub size: f32,
    /// Show/hide progress in `0..=1` (animated). `0` = hidden, `1` = shown.
    pub visible_progress: f32,
    /// Resize-handle (gutter) thickness when content is shown.
    pub gutter: f32,
    /// Minimum content thickness (used only by the caller's `layout_response`).
    pub min_size: f32,
    /// Always-visible rail (activity bar) thickness; `0` when the side has
    /// no rail.
    pub rail_thickness: f32,
    /// Whether this side shows an activity rail.
    pub has_rail: bool,
}

impl SideLayout {
    /// A fully-collapsed, rail-less, zero-size placeholder.
    pub fn empty() -> Self {
        Self {
            size: 0.0,
            visible_progress: 0.0,
            gutter: 0.0,
            min_size: 0.0,
            rail_thickness: 0.0,
            has_rail: false,
        }
    }

    fn rail_extent(&self) -> f32 {
        if self.has_rail {
            self.rail_thickness.max(0.0)
        } else {
            0.0
        }
    }

    fn content_extent(&self) -> f32 {
        (self.size * self.visible_progress.clamp(0.0, 1.0)).max(0.0)
    }

    fn gutter_extent(&self) -> f32 {
        if self.content_extent() > EPS {
            self.gutter.max(0.0)
        } else {
            0.0
        }
    }

    /// Total extent the side occupies toward the centre (rail + content +
    /// gutter). Used for the **leading / trailing** columns, where the rail sits
    /// on the main (thickness) axis.
    fn total_extent(&self) -> f32 {
        self.rail_extent() + self.content_extent() + self.gutter_extent()
    }

    /// Depth a **top / bottom** band occupies toward the centre. Their (vertical)
    /// rail sits on the *cross* (leading) edge as a column, so it does **not**
    /// add to the band depth — that's content + gutter. A hidden top / bottom
    /// band collapses **completely** (the vertical rail can't stand alone in a
    /// zero-depth band the way a leading/trailing rail can in a full-height
    /// column) — the app offers an external button to reveal it again.
    fn band_depth(&self) -> f32 {
        self.content_extent() + self.gutter_extent()
    }

    /// Whether a leading / trailing column occupies any space.
    fn present(&self) -> bool {
        self.total_extent() > EPS
    }

    /// Whether a top / bottom band occupies any space.
    fn band_present(&self) -> bool {
        self.band_depth() > EPS
    }
}

/// The three sub-rectangles a side contributes: the always-visible rail, the
/// resizable content, and the resize handle. Any of them is [`Rect::ZERO`]
/// when absent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideRects {
    pub rail: Rect,
    pub content: Rect,
    pub handle: Rect,
}

impl SideRects {
    const ZERO: SideRects = SideRects {
        rail: Rect::ZERO,
        content: Rect::ZERO,
        handle: Rect::ZERO,
    };
}

/// The computed geometry: four side breakdowns plus the centre rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockingRects {
    pub leading: SideRects,
    pub trailing: SideRects,
    pub top: SideRects,
    pub bottom: SideRects,
    pub center: Rect,
}

/// Resolve a corner to its *effective* owner: the declared owner if that side
/// is present, else the other adjacent side if *it* is present, else the
/// declared owner (both absent — the choice is moot).
fn effective_owner(
    corner: DockCorner,
    owners: &CornerOwners,
    present: impl Fn(DockSide) -> bool,
) -> DockSide {
    let declared = owners.owner(corner);
    if present(declared) {
        declared
    } else {
        let other = corner.other(declared);
        if present(other) { other } else { declared }
    }
}

/// Compute the five region rectangles. The caller swaps `leading` and
/// `trailing` on the way in and the resulting `leading`/`trailing` on the way
/// out for RTL; `rtl` is passed through only to place a top / bottom band's
/// (vertical) rail on the leading cross-edge (left in LTR, right in RTL).
pub fn compute_rects(
    container: Rect,
    leading: SideLayout,
    trailing: SideLayout,
    top: SideLayout,
    bottom: SideLayout,
    owners: CornerOwners,
    rtl: bool,
) -> DockingRects {
    let x = container.x;
    let y = container.y;
    let w = container.width.max(0.0);
    let h = container.height.max(0.0);

    // Total extents per side, pre-clamped so opposing sides can never claim
    // more than the container in either axis (centre shrinks to zero first).
    let mut l = leading.total_extent();
    let mut r = trailing.total_extent();
    let mut t = top.band_depth();
    let mut b = bottom.band_depth();
    if l + r > w {
        let scale = if l + r > 0.0 { w / (l + r) } else { 0.0 };
        l *= scale;
        r *= scale;
    }
    if t + b > h {
        let scale = if t + b > 0.0 { h / (t + b) } else { 0.0 };
        t *= scale;
        b *= scale;
    }

    let present = |side: DockSide| match side {
        DockSide::Leading => leading.present(),
        DockSide::Trailing => trailing.present(),
        DockSide::Top => top.band_present(),
        DockSide::Bottom => bottom.band_present(),
    };
    let eff = |corner: DockCorner| effective_owner(corner, &owners, present);

    // Horizontal extents of the top / bottom bands.
    let top_x_left = if eff(DockCorner::TopLeading) == DockSide::Top {
        x
    } else {
        x + l
    };
    let top_x_right = if eff(DockCorner::TopTrailing) == DockSide::Top {
        x + w
    } else {
        x + w - r
    };
    let bottom_x_left = if eff(DockCorner::BottomLeading) == DockSide::Bottom {
        x
    } else {
        x + l
    };
    let bottom_x_right = if eff(DockCorner::BottomTrailing) == DockSide::Bottom {
        x + w
    } else {
        x + w - r
    };

    // Vertical extents of the leading / trailing columns.
    let leading_y_top = if eff(DockCorner::TopLeading) == DockSide::Leading {
        y
    } else {
        y + t
    };
    let leading_y_bottom = if eff(DockCorner::BottomLeading) == DockSide::Leading {
        y + h
    } else {
        y + h - b
    };
    let trailing_y_top = if eff(DockCorner::TopTrailing) == DockSide::Trailing {
        y
    } else {
        y + t
    };
    let trailing_y_bottom = if eff(DockCorner::BottomTrailing) == DockSide::Trailing {
        y + h
    } else {
        y + h - b
    };

    // Outer region rects (the whole side band).
    let leading_region = Rect::new(
        x,
        leading_y_top,
        l,
        (leading_y_bottom - leading_y_top).max(0.0),
    );
    let trailing_region = Rect::new(
        x + w - r,
        trailing_y_top,
        r,
        (trailing_y_bottom - trailing_y_top).max(0.0),
    );
    let top_region = Rect::new(top_x_left, y, (top_x_right - top_x_left).max(0.0), t);
    let bottom_region = Rect::new(
        bottom_x_left,
        y + h - b,
        (bottom_x_right - bottom_x_left).max(0.0),
        b,
    );
    let center = Rect::new(x + l, y + t, (w - l - r).max(0.0), (h - t - b).max(0.0));

    DockingRects {
        leading: split_side(DockSide::Leading, leading_region, &leading, l, rtl),
        trailing: split_side(DockSide::Trailing, trailing_region, &trailing, r, rtl),
        top: split_side(DockSide::Top, top_region, &top, t, rtl),
        bottom: split_side(DockSide::Bottom, bottom_region, &bottom, b, rtl),
        center,
    }
}

/// Split a side's outer region into rail / content / handle sub-rects.
///
/// Leading / trailing lay them out along the thickness axis (`[rail │ content │
/// handle]`), the rail on the outer edge. Top / bottom put the (vertical) rail
/// as a **column on the leading cross-edge** (left in LTR, right in RTL) and
/// split content / handle along the band depth to its inboard side. `total` is
/// the (possibly clamped) band extent.
fn split_side(
    side: DockSide,
    region: Rect,
    layout: &SideLayout,
    total: f32,
    rtl: bool,
) -> SideRects {
    if total <= EPS || region.width <= 0.0 || region.height <= 0.0 {
        // Rail-only sides still get a rail rect when there is room.
        if layout.rail_extent() > EPS && region.width > 0.0 && region.height > 0.0 {
            return rail_only(
                side,
                region,
                layout.rail_extent().min(extent_along(side, region)),
                rtl,
            );
        }
        return SideRects::ZERO;
    }

    match side {
        DockSide::Leading | DockSide::Trailing => {
            // The rail is on the main (thickness) axis; rail + content + gutter
            // share the band width, scaled to fit `total`.
            let raw = layout.total_extent();
            let scale = if raw > 0.0 { total / raw } else { 0.0 };
            let rail = layout.rail_extent() * scale;
            let content = layout.content_extent() * scale;
            let gutter = layout.gutter_extent() * scale;
            match side {
                DockSide::Leading => SideRects {
                    rail: Rect::new(region.x, region.y, rail, region.height),
                    content: Rect::new(region.x + rail, region.y, content, region.height),
                    handle: Rect::new(region.x + rail + content, region.y, gutter, region.height),
                },
                _ => SideRects {
                    handle: Rect::new(region.x, region.y, gutter, region.height),
                    content: Rect::new(region.x + gutter, region.y, content, region.height),
                    rail: Rect::new(region.x + gutter + content, region.y, rail, region.height),
                },
            }
        }
        DockSide::Top | DockSide::Bottom => {
            // Vertical rail = a column on the leading cross-edge; content + handle
            // fill the rest, split along the band depth (`total`).
            let rail_w = layout.rail_extent().min(region.width);
            let body_w = (region.width - rail_w).max(0.0);
            // Leading edge: left in LTR, right in RTL.
            let (rail_x, body_x) = if rtl {
                (region.x + body_w, region.x)
            } else {
                (region.x, region.x + rail_w)
            };
            let raw_depth = layout.content_extent() + layout.gutter_extent();
            let scale = if raw_depth > 0.0 {
                total / raw_depth
            } else {
                0.0
            };
            let content = layout.content_extent() * scale;
            let gutter = layout.gutter_extent() * scale;
            let rail = Rect::new(rail_x, region.y, rail_w, region.height);
            match side {
                // Top: content on top, handle below it (inboard, toward centre).
                DockSide::Top => SideRects {
                    rail,
                    content: Rect::new(body_x, region.y, body_w, content),
                    handle: Rect::new(body_x, region.y + content, body_w, gutter),
                },
                // Bottom: handle on top (inboard, toward centre), content below.
                _ => SideRects {
                    rail,
                    handle: Rect::new(body_x, region.y, body_w, gutter),
                    content: Rect::new(body_x, region.y + gutter, body_w, content),
                },
            }
        }
    }
}

fn extent_along(side: DockSide, region: Rect) -> f32 {
    if side.is_horizontal_axis() {
        region.width
    } else {
        region.height
    }
}

/// A side with only its rail present (content hidden). Leading / trailing rails
/// hug the outer thickness edge; top / bottom rails are a column on the leading
/// cross-edge (left in LTR, right in RTL).
fn rail_only(side: DockSide, region: Rect, rail: f32, rtl: bool) -> SideRects {
    let mut rects = SideRects::ZERO;
    rects.rail = match side {
        DockSide::Leading => Rect::new(region.x, region.y, rail, region.height),
        DockSide::Trailing => Rect::new(
            region.x + region.width - rail,
            region.y,
            rail,
            region.height,
        ),
        DockSide::Top | DockSide::Bottom => {
            let rail_x = if rtl {
                region.x + region.width - rail
            } else {
                region.x
            };
            Rect::new(rail_x, region.y, rail, region.height)
        }
    };
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container() -> Rect {
        Rect::new(0.0, 0.0, 1000.0, 800.0)
    }

    /// A simple shown side: content `size`, no rail.
    fn shown(size: f32) -> SideLayout {
        SideLayout {
            size,
            visible_progress: 1.0,
            gutter: 6.0,
            min_size: 50.0,
            rail_thickness: 0.0,
            has_rail: false,
        }
    }

    fn hidden(size: f32) -> SideLayout {
        SideLayout {
            visible_progress: 0.0,
            ..shown(size)
        }
    }

    #[test]
    fn all_hidden_center_fills() {
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert_eq!(r.center, container());
    }

    #[test]
    fn single_leading_side() {
        let r = compute_rects(
            container(),
            shown(200.0),
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert_eq!(r.leading.content.x, 0.0);
        assert!((r.leading.content.width - 200.0).abs() < 0.01);
        assert!(
            (r.center.x - 206.0).abs() < 0.01,
            "center after content+gutter"
        );
        assert!((r.center.width - (1000.0 - 206.0)).abs() < 0.01);
        assert_eq!(r.trailing, SideRects::ZERO);
    }

    #[test]
    fn four_sides_default_corners_inset_center() {
        let r = compute_rects(
            container(),
            shown(200.0),
            shown(150.0),
            shown(100.0),
            shown(120.0),
            CornerOwners::default(),
            false,
        );
        // Default: top/bottom own corners → they span full width.
        assert_eq!(r.top.content.x, 0.0);
        assert!((r.top.content.width - 1000.0).abs() < 0.01);
        assert_eq!(r.bottom.content.x, 0.0);
        // Leading column is inset vertically by top + gutter and bottom + gutter.
        assert!((r.leading.content.y - 106.0).abs() < 0.01);
        // Center inset on all four sides (size + gutter each).
        assert!((r.center.x - 206.0).abs() < 0.01);
        assert!((r.center.y - 106.0).abs() < 0.01);
    }

    #[test]
    fn corner_owned_by_leading_extends_column_up() {
        let mut owners = CornerOwners::default();
        owners.set(DockCorner::TopLeading, DockSide::Leading);
        let r = compute_rects(
            container(),
            shown(200.0),
            SideLayout::empty(),
            shown(100.0),
            SideLayout::empty(),
            owners,
            false,
        );
        // Leading now extends to y=0; top starts after the leading column.
        assert_eq!(r.leading.content.y, 0.0);
        assert!(
            (r.top.content.x - 206.0).abs() < 0.01,
            "top pushed right of leading"
        );
    }

    #[test]
    fn corner_degrades_when_owner_hidden() {
        // TopLeading declared to Leading, but leading is hidden ⇒ top fills.
        let mut owners = CornerOwners::default();
        owners.set(DockCorner::TopLeading, DockSide::Leading);
        let r = compute_rects(
            container(),
            hidden(200.0),
            SideLayout::empty(),
            shown(100.0),
            SideLayout::empty(),
            owners,
            false,
        );
        assert_eq!(
            r.top.content.x, 0.0,
            "top fills since its corner-owner is gone"
        );
    }

    #[test]
    fn visible_progress_half_scales_content() {
        let mut s = shown(200.0);
        s.visible_progress = 0.5;
        let r = compute_rects(
            container(),
            s,
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert!((r.leading.content.width - 100.0).abs() < 0.01);
        assert!(
            r.leading.handle.width > 0.0,
            "gutter present while expanding"
        );
    }

    #[test]
    fn hidden_side_with_rail_keeps_rail_drops_handle() {
        let s = SideLayout {
            size: 240.0,
            visible_progress: 0.0,
            gutter: 6.0,
            min_size: 60.0,
            rail_thickness: 48.0,
            has_rail: true,
        };
        let r = compute_rects(
            container(),
            s,
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert!((r.leading.rail.width - 48.0).abs() < 0.01, "rail persists");
        assert!(r.leading.content.width.abs() < 0.01, "content hidden");
        assert!(r.leading.handle.width.abs() < 0.01, "no handle when hidden");
        assert!(
            (r.center.x - 48.0).abs() < 0.01,
            "center inset only by rail"
        );
    }

    #[test]
    fn shown_side_with_rail_orders_rail_content_handle() {
        let s = SideLayout {
            size: 200.0,
            visible_progress: 1.0,
            gutter: 6.0,
            min_size: 60.0,
            rail_thickness: 48.0,
            has_rail: true,
        };
        let r = compute_rects(
            container(),
            s,
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert_eq!(r.leading.rail.x, 0.0);
        assert!((r.leading.rail.width - 48.0).abs() < 0.01);
        assert!((r.leading.content.x - 48.0).abs() < 0.01);
        assert!((r.leading.content.width - 200.0).abs() < 0.01);
        assert!((r.leading.handle.x - 248.0).abs() < 0.01);
        assert!((r.center.x - 254.0).abs() < 0.01);
    }

    /// A top/bottom side with a rail.
    fn band_with_rail(progress: f32) -> SideLayout {
        SideLayout {
            size: 100.0,
            visible_progress: progress,
            gutter: 6.0,
            min_size: 80.0,
            rail_thickness: 48.0,
            has_rail: true,
        }
    }

    #[test]
    fn top_rail_is_a_leading_column_not_a_band() {
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            SideLayout::empty(),
            band_with_rail(1.0),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        // Rail is a vertical column on the leading (left) edge, spanning the
        // band depth (content + gutter = 106), NOT a horizontal band.
        assert_eq!(r.top.rail.x, 0.0);
        assert!((r.top.rail.width - 48.0).abs() < 0.01);
        assert!((r.top.rail.height - 106.0).abs() < 0.01);
        // Content is inboard to the right of the rail.
        assert!((r.top.content.x - 48.0).abs() < 0.01);
        assert!((r.top.content.width - (1000.0 - 48.0)).abs() < 0.01);
        assert!((r.top.content.height - 100.0).abs() < 0.01);
        // The rail does not push the centre down — only content + gutter do.
        assert!((r.center.y - 106.0).abs() < 0.01);
    }

    #[test]
    fn top_rail_column_mirrors_to_the_right_in_rtl() {
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            SideLayout::empty(),
            band_with_rail(1.0),
            SideLayout::empty(),
            CornerOwners::default(),
            true,
        );
        assert!(
            (r.top.rail.x - (1000.0 - 48.0)).abs() < 0.01,
            "rail on the right in RTL"
        );
        assert_eq!(r.top.content.x, 0.0, "content on the left in RTL");
    }

    #[test]
    fn hidden_top_with_rail_fully_collapses() {
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            SideLayout::empty(),
            band_with_rail(0.0),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        // A hidden top/bottom band hides its vertical rail too (no persistent
        // column) — the app reveals it again via an external button.
        assert!(r.top.rail.height.abs() < 0.01, "no rail column when hidden");
        assert!(r.top.content.height.abs() < 0.01, "no content when hidden");
        assert_eq!(r.center.y, 0.0, "centre fills — no top inset");
        assert!((r.center.height - 800.0).abs() < 0.01);
    }

    #[test]
    fn bottom_rail_column_keeps_handle_inboard() {
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            SideLayout::empty(),
            SideLayout::empty(),
            band_with_rail(1.0),
            CornerOwners::default(),
            false,
        );
        // Rail column on the leading edge; band pinned to the container bottom.
        assert_eq!(r.bottom.rail.x, 0.0);
        assert!((r.bottom.rail.width - 48.0).abs() < 0.01);
        // The resize handle sits at the band's TOP (inboard, toward centre),
        // right of the rail; content is below it.
        assert!((r.bottom.handle.x - 48.0).abs() < 0.01);
        assert!((r.bottom.handle.y - (800.0 - 106.0)).abs() < 0.01);
        assert!(
            r.bottom.content.y > r.bottom.handle.y,
            "content below the inboard handle"
        );
    }

    #[test]
    fn trailing_rail_sits_on_the_outer_edge() {
        let s = SideLayout {
            size: 200.0,
            visible_progress: 1.0,
            gutter: 6.0,
            min_size: 60.0,
            rail_thickness: 48.0,
            has_rail: true,
        };
        let r = compute_rects(
            container(),
            SideLayout::empty(),
            s,
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        // Trailing band: handle | content | rail, rail flush to the right edge.
        assert!((r.trailing.rail.right() - 1000.0).abs() < 0.01);
        assert!((r.trailing.rail.width - 48.0).abs() < 0.01);
        assert!(r.trailing.handle.x < r.trailing.content.x);
        assert!(r.trailing.content.x < r.trailing.rail.x);
    }

    #[test]
    fn handle_spans_same_cross_extent_as_content() {
        let r = compute_rects(
            container(),
            shown(200.0),
            SideLayout::empty(),
            shown(100.0),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        assert!((r.leading.handle.height - r.leading.content.height).abs() < 0.01);
        assert!((r.leading.handle.y - r.leading.content.y).abs() < 0.01);
    }

    #[test]
    fn center_never_negative_under_over_constraint() {
        // Sides demand far more than the container.
        let big = shown(900.0);
        let r = compute_rects(
            container(),
            big,
            big,
            big,
            big,
            CornerOwners::default(),
            false,
        );
        assert!(r.center.width >= 0.0);
        assert!(r.center.height >= 0.0);
        // No band exceeds the container.
        assert!(r.leading.content.width + r.trailing.content.width <= 1000.0 + 0.01);
    }

    #[test]
    fn zero_by_zero_container_no_panic() {
        let r = compute_rects(
            Rect::new(0.0, 0.0, 0.0, 0.0),
            shown(200.0),
            shown(200.0),
            shown(100.0),
            shown(100.0),
            CornerOwners::default(),
            false,
        );
        assert_eq!(r.center, Rect::new(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn degenerate_corner_clamps_to_zero() {
        // Top + bottom exceed the height with leading owning both side
        // corners → leading column height clamps to >= 0, no panic.
        let mut owners = CornerOwners::default();
        owners.set(DockCorner::TopLeading, DockSide::Leading);
        owners.set(DockCorner::BottomLeading, DockSide::Leading);
        let r = compute_rects(
            Rect::new(0.0, 0.0, 1000.0, 100.0),
            shown(200.0),
            SideLayout::empty(),
            shown(80.0),
            shown(80.0),
            owners,
            false,
        );
        assert!(r.leading.content.height >= 0.0);
    }

    #[test]
    fn idempotent() {
        let a = compute_rects(
            container(),
            shown(200.0),
            shown(150.0),
            shown(100.0),
            shown(120.0),
            CornerOwners::default(),
            false,
        );
        let b = compute_rects(
            container(),
            shown(200.0),
            shown(150.0),
            shown(100.0),
            shown(120.0),
            CornerOwners::default(),
            false,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn corner_other_side_helper() {
        assert_eq!(
            DockCorner::TopLeading.other(DockSide::Leading),
            DockSide::Top
        );
        assert_eq!(
            DockCorner::TopLeading.other(DockSide::Top),
            DockSide::Leading
        );
    }

    #[test]
    fn rtl_mirror_is_caller_swap() {
        // The engine is LTR-only; the caller swaps leading/trailing. Verify a
        // swapped call mirrors the bands.
        let ltr = compute_rects(
            container(),
            shown(200.0),
            shown(150.0),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        let rtl = compute_rects(
            container(),
            shown(150.0),
            shown(200.0),
            SideLayout::empty(),
            SideLayout::empty(),
            CornerOwners::default(),
            false,
        );
        // In RTL the (logical) leading band has trailing's geometry mirrored.
        assert!((ltr.leading.content.width - rtl.trailing.content.width).abs() < 0.01);
    }
}
