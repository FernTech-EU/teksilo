//! Accessibility policies for `SceneView`.
//!
//! Phase 5a ships the **visual-default** path: an off-screen-mode
//! enum + helper that decides which items the AT walker should
//! emit. Phase 5b layers the parallel-structural API on top
//! (logical groups, parents, relations, auto-graft, custom focus
//! callbacks) — see `docs/fern-scene-a11y.md` for the full picture.
//!
//! Defaults are chosen so a quick prototype is accessible out of
//! the box: every visible heavyweight widget participates in the
//! AT walker as a normal direct child of `SceneView`, every visible
//! lightweight item gets a synthetic AT node with role +
//! screen-projected bounds, and Tab cycles in reading order.

use fern_canvas::Rect;

/// Off-screen visibility policy for the AT walker. Decides which
/// scene items get emitted as synthetic AT nodes per AT-rebuild.
///
/// `ViewportPlusN { n: 1 }` is the default: an item appears in the
/// AT tree if its `bounds_in_scene` intersects `viewport ∪ (1×
/// viewport-grown-rect)`. That keeps the tree close to "what the
/// user can interact with right now" while letting screen-reader
/// users discover items just outside the visible region by jumping
/// to the next/prev — at which point `SceneView::focus_item` auto-
/// pans the view to bring the focused item into view (Phase 5+).
#[derive(Debug, Clone, Copy)]
pub enum A11yOffScreenMode {
    /// Emit *every* item in the scene as a synthetic AT node.
    /// Heaviest mode — appropriate for small scenes (< ~500 items)
    /// where AT users want a complete table of contents.
    AllItems,

    /// Emit items inside the viewport plus an `n × viewport`-grown
    /// margin around it. `n = 0` collapses to "viewport only" with
    /// the same allocation pattern as `ViewportOnly`. `n = 1` is
    /// the default — gives screen-reader users a one-screen
    /// "lookahead" to navigate without `focus_item` round-tripping
    /// through pan animation.
    ViewportPlusN { n: u32 },

    /// Strict: only items intersecting the current viewport. Pairs
    /// with apps that have very large scenes where listing
    /// off-screen content would overwhelm AT clients.
    ViewportOnly,
}

impl A11yOffScreenMode {
    /// Compute the scene-coord rectangle a given mode considers
    /// "AT-visible" given the current visible scene region. Used by
    /// `SceneView::accessibility` as the spatial-index query rect.
    /// `AllItems` returns `None` so the caller knows to bypass the
    /// query and emit every item.
    pub fn at_visible_region(&self, visible_scene_region: Rect) -> Option<Rect> {
        match *self {
            A11yOffScreenMode::AllItems => None,
            A11yOffScreenMode::ViewportOnly => Some(visible_scene_region),
            A11yOffScreenMode::ViewportPlusN { n } => {
                if n == 0 {
                    return Some(visible_scene_region);
                }
                let margin_x = visible_scene_region.width * n as f32;
                let margin_y = visible_scene_region.height * n as f32;
                Some(Rect::new(
                    visible_scene_region.x - margin_x,
                    visible_scene_region.y - margin_y,
                    visible_scene_region.width + margin_x * 2.0,
                    visible_scene_region.height + margin_y * 2.0,
                ))
            }
        }
    }
}

impl Default for A11yOffScreenMode {
    fn default() -> Self {
        A11yOffScreenMode::ViewportPlusN { n: 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_viewport_plus_one() {
        assert!(matches!(
            A11yOffScreenMode::default(),
            A11yOffScreenMode::ViewportPlusN { n: 1 }
        ));
    }

    #[test]
    fn all_items_returns_none() {
        assert_eq!(
            A11yOffScreenMode::AllItems
                .at_visible_region(Rect::new(0.0, 0.0, 100.0, 100.0)),
            None
        );
    }

    #[test]
    fn viewport_only_passes_through() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(
            A11yOffScreenMode::ViewportOnly.at_visible_region(viewport),
            Some(viewport)
        );
    }

    #[test]
    fn viewport_plus_n_grows_symmetrically() {
        // Viewport at (0,0)-(100,80), n=1 → grow by ±100 in x, ±80
        // in y → final rect (-100,-80)-(200,160) i.e. 300×240.
        let viewport = Rect::new(0.0, 0.0, 100.0, 80.0);
        let grown = A11yOffScreenMode::ViewportPlusN { n: 1 }
            .at_visible_region(viewport)
            .unwrap();
        assert_eq!(grown, Rect::new(-100.0, -80.0, 300.0, 240.0));
    }

    #[test]
    fn viewport_plus_zero_equals_viewport_only() {
        let viewport = Rect::new(50.0, 50.0, 200.0, 100.0);
        assert_eq!(
            A11yOffScreenMode::ViewportPlusN { n: 0 }.at_visible_region(viewport),
            A11yOffScreenMode::ViewportOnly.at_visible_region(viewport),
        );
    }

    #[test]
    fn viewport_plus_two_grows_by_two_viewports_each_side() {
        let viewport = Rect::new(0.0, 0.0, 100.0, 100.0);
        let grown = A11yOffScreenMode::ViewportPlusN { n: 2 }
            .at_visible_region(viewport)
            .unwrap();
        // Margin is 200 on each side → final 500×500.
        assert_eq!(grown, Rect::new(-200.0, -200.0, 500.0, 500.0));
    }
}
