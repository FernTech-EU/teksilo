// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Safe-triangle submenu hover-gate geometry.
//!
//! When a submenu opens, the user moves the pointer diagonally
//! toward it. With pure delay-based hover-open suppression, a brief
//! pause anywhere on the path closes the submenu — which feels
//! wrong. The "safe triangle" approach instead defines a triangle
//! with:
//!
//! - **apex** = pointer position at the moment the submenu opened
//! - **base** = the submenu's **near** vertical edge (the edge facing
//!   the parent menu, irrespective of LTR / RTL — we read it off the
//!   submenu's actual screen rect)
//!
//! As long as the pointer stays inside this triangle, sibling
//! hover-opens are suppressed: the user is "still heading there".
//! The moment the pointer leaves the triangle (or stays out for the
//! existing PointerLeave grace period), the timer-based close
//! fallback runs as before.
//!
//! The algorithm is RTL-symmetric **automatically** because the
//! "near edge" is inferred from `anchor.x` vs `submenu.x`, not from
//! a hardcoded `Leading` / `Trailing` enum. A submenu opened to the
//! left of its parent (RTL, or LTR with no room right) flips the
//! triangle's near edge to the submenu's right edge without any
//! code change.
//!
//! This is the algorithm every desktop menu has used since the
//! Macintosh Toolbox.

use bastyde_canvas::{Point, Rect};

/// Inclusive point-in-triangle test using the standard 3-sign
/// cross-product check. `apex` is the pointer-at-submenu-open
/// anchor; `submenu` is the open submenu's screen rect.
///
/// Returns `false` for degenerate inputs (zero-area submenu, or the
/// apex sits exactly on the near-edge line — defensive choice; the
/// gate falls back to the existing delay-based dismiss).
pub(crate) fn point_in_safe_triangle(p: Point, apex: Point, submenu: Rect) -> bool {
    if submenu.width <= 0.0 || submenu.height <= 0.0 {
        return false;
    }
    // Decide which vertical edge of `submenu` is the "near" edge
    // based on where the apex sits relative to the submenu. If the
    // apex is to the left of the submenu, the near edge is the
    // submenu's left edge (LTR case). If the apex is to the right,
    // the near edge is the submenu's right edge (RTL case, or LTR
    // with no room on the right).
    let near_x = if apex.x < submenu.x {
        submenu.x
    } else if apex.x > submenu.x + submenu.width {
        submenu.x + submenu.width
    } else {
        // Apex is inside the submenu's horizontal extent — there's
        // no meaningful near edge. Treat as outside.
        return false;
    };
    let top = Point::new(near_x, submenu.y);
    let bottom = Point::new(near_x, submenu.y + submenu.height);
    point_in_triangle(p, apex, top, bottom)
}

/// Inclusive point-in-triangle via the 3-sign cross-product test.
/// Vertices in any winding order.
fn point_in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let s1 = cross(p, a, b);
    let s2 = cross(p, b, c);
    let s3 = cross(p, c, a);
    let neg = (s1 < 0.0) || (s2 < 0.0) || (s3 < 0.0);
    let pos = (s1 > 0.0) || (s2 > 0.0) || (s3 > 0.0);
    !(neg && pos)
}

/// Sign of the cross-product of (b - a) and (p - a). Used by the
/// point-in-triangle test.
fn cross(p: Point, a: Point, b: Point) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submenu_to_the_right() -> Rect {
        // Parent menu sits roughly at x=0..160; submenu opens to its right.
        Rect::new(160.0, 100.0, 200.0, 240.0)
    }

    fn submenu_to_the_left() -> Rect {
        // Mirrored layout — parent at x=200..360, submenu opens left.
        Rect::new(0.0, 100.0, 200.0, 240.0)
    }

    #[test]
    fn apex_inside_triangle_returns_true() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 200.0); // somewhere over the parent
        // The apex itself is on the triangle (a vertex).
        assert!(point_in_safe_triangle(apex, apex, sub));
    }

    #[test]
    fn pointer_directly_toward_submenu_top_corner_is_inside() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 220.0);
        let midpoint_toward_top_left = Point::new(120.0, 160.0);
        assert!(point_in_safe_triangle(midpoint_toward_top_left, apex, sub));
    }

    #[test]
    fn pointer_directly_toward_submenu_bottom_corner_is_inside() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 220.0);
        let midpoint_toward_bottom_left = Point::new(120.0, 280.0);
        assert!(point_in_safe_triangle(
            midpoint_toward_bottom_left,
            apex,
            sub
        ));
    }

    #[test]
    fn pointer_above_apex_outside() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 220.0);
        // Way above the apex — well outside the cone.
        assert!(!point_in_safe_triangle(Point::new(80.0, 50.0), apex, sub));
    }

    #[test]
    fn pointer_below_apex_outside() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 220.0);
        assert!(!point_in_safe_triangle(Point::new(80.0, 400.0), apex, sub));
    }

    #[test]
    fn pointer_left_of_apex_outside_when_submenu_is_right() {
        let sub = submenu_to_the_right();
        let apex = Point::new(80.0, 220.0);
        assert!(!point_in_safe_triangle(Point::new(0.0, 220.0), apex, sub));
    }

    // --- RTL mirror ---

    #[test]
    fn submenu_on_left_pointer_toward_top_corner_is_inside() {
        // RTL layout: submenu opens to the left of the apex.
        let sub = submenu_to_the_left();
        let apex = Point::new(280.0, 220.0);
        // Pointer travelling diagonally toward the submenu's
        // top-right (near) corner.
        let p = Point::new(220.0, 160.0);
        assert!(point_in_safe_triangle(p, apex, sub));
    }

    #[test]
    fn submenu_on_left_pointer_drifting_further_left_is_outside() {
        // RTL layout: once the user has clearly walked past the
        // submenu's right (near) edge, no more gating.
        let sub = submenu_to_the_left();
        let apex = Point::new(280.0, 220.0);
        let p = Point::new(-50.0, 220.0);
        assert!(!point_in_safe_triangle(p, apex, sub));
    }

    #[test]
    fn submenu_on_left_pointer_right_of_apex_outside() {
        let sub = submenu_to_the_left();
        let apex = Point::new(280.0, 220.0);
        // Walking right (away from the submenu) — outside the cone.
        assert!(!point_in_safe_triangle(Point::new(360.0, 220.0), apex, sub));
    }

    // --- Degenerate cases ---

    #[test]
    fn zero_width_submenu_returns_false() {
        let sub = Rect::new(160.0, 100.0, 0.0, 240.0);
        let apex = Point::new(80.0, 220.0);
        assert!(!point_in_safe_triangle(Point::new(120.0, 220.0), apex, sub));
    }

    #[test]
    fn zero_height_submenu_returns_false() {
        let sub = Rect::new(160.0, 100.0, 200.0, 0.0);
        let apex = Point::new(80.0, 220.0);
        assert!(!point_in_safe_triangle(Point::new(220.0, 100.0), apex, sub));
    }

    #[test]
    fn apex_inside_submenu_horizontal_extent_returns_false() {
        // The apex sitting *inside* the submenu's left/right span has
        // no meaningful "near edge" — the gate falls through to the
        // existing dismiss timer.
        let sub = submenu_to_the_right();
        let apex_inside_sub_x = Point::new(180.0, 50.0);
        assert!(!point_in_safe_triangle(
            Point::new(180.0, 200.0),
            apex_inside_sub_x,
            sub
        ));
    }
}
