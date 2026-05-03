//! [`SceneViewState`] — a snapshot of a [`SceneView`](crate::SceneView)'s
//! pan / zoom / rotation, suitable for persistence between sessions.
//!
//! ## Pattern
//!
//! ```ignore
//! use fern_scene::{Scene, SceneView, SceneViewState};
//!
//! // On load: read from your persistence layer (fern-settings,
//! // a custom JSON file, etc.) and pass to SceneView.
//! let saved: SceneViewState = my_settings.scene_view.get();
//! let view = SceneView::new(scene);
//! view.restore_state(saved);
//!
//! // On exit / periodic flush: snapshot and persist.
//! let current: SceneViewState = view.state();
//! my_settings.scene_view.set(current);
//! ```
//!
//! ## Why a plain struct, not Serialize
//!
//! `fern-scene` deliberately doesn't depend on `serde`. Apps that
//! want to persist via `fern-settings` (which is `serde`-based)
//! either:
//!
//! - Add their own newtype wrapper that implements
//!   `Serialize / Deserialize`, OR
//! - Store the fields individually (`pan_x`, `pan_y`, `zoom`,
//!   `rotation`) as scalar `SettingsKey<f32>`s in a
//!   `SettingsStore`.
//!
//! The struct is plain-old-data — manual round-trip is trivial.

use fern_canvas::Vec2;

/// Snapshot of a SceneView's view transform: pan offset, zoom
/// factor, and rotation in radians. Use [`SceneView::state`] to
/// capture the current values; [`SceneView::restore_state`] to
/// apply a saved snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneViewState {
    /// X pan offset (the same value driven by the SceneView's
    /// `pan_x` signal).
    pub pan_x: f32,
    /// Y pan offset.
    pub pan_y: f32,
    /// Zoom factor (1.0 = identity).
    pub zoom: f32,
    /// Rotation in radians (0.0 = no rotation).
    pub rotation: f32,
}

impl SceneViewState {
    /// The identity view state: no pan, zoom 1.0, no rotation.
    pub const IDENTITY: SceneViewState = SceneViewState {
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
        rotation: 0.0,
    };

    /// Construct a new state with the given pan / zoom / rotation.
    pub fn new(pan: Vec2, zoom: f32, rotation: f32) -> Self {
        Self {
            pan_x: pan.x,
            pan_y: pan.y,
            zoom,
            rotation,
        }
    }

    /// Pan offset as a [`Vec2`].
    pub fn pan(&self) -> Vec2 {
        Vec2::new(self.pan_x, self.pan_y)
    }

    /// Whether this state is the identity (no pan, zoom 1.0, no
    /// rotation). Useful for skipping persistence of fresh-default
    /// SceneViews.
    pub fn is_identity(&self) -> bool {
        self.pan_x == 0.0 && self.pan_y == 0.0 && self.zoom == 1.0 && self.rotation == 0.0
    }
}

impl Default for SceneViewState {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trip() {
        let s = SceneViewState::IDENTITY;
        assert!(s.is_identity());
        assert_eq!(s.pan(), Vec2::ZERO);
        assert_eq!(s.zoom, 1.0);
        assert_eq!(s.rotation, 0.0);
    }

    #[test]
    fn non_identity_state_constructs_correctly() {
        let s = SceneViewState::new(Vec2::new(10.0, 20.0), 1.5, 0.1);
        assert!(!s.is_identity());
        assert_eq!(s.pan_x, 10.0);
        assert_eq!(s.pan_y, 20.0);
        assert_eq!(s.zoom, 1.5);
        assert_eq!(s.rotation, 0.1);
    }
}
