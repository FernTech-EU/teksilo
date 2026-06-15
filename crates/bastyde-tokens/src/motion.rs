// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Easing curve for animations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    #[default]
    EaseOut,
    EaseInOut,
}

impl Easing {
    /// Apply the easing curve to a linear progress value `t` in [0, 1].
    /// Returns the eased value, also in [0, 1].
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
        }
    }
}

/// Linearly interpolate between two f32 values.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Motion tokens — Int UI durations and easing.
///
/// Int UI's philosophy: hover and press are **instant**; animation is reserved
/// for floating elements (tooltips, balloons, dialogs). JetBrains explicitly
/// avoids decorative animation. A single mild ease-out is used everywhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTokens {
    /// 0 ms — most state changes (hover, press).
    #[serde(with = "duration_millis")]
    pub duration_instant: Duration,
    /// ~120 ms — tooltip fade, popup fade.
    #[serde(with = "duration_millis")]
    pub duration_fast: Duration,
    /// ~200 ms — notification balloon slide.
    #[serde(with = "duration_millis")]
    pub duration_normal: Duration,
    /// ~300 ms — dialog appearance.
    #[serde(with = "duration_millis")]
    pub duration_slow: Duration,
    /// ~200 ms — accordion / disclosure expand-collapse height tween.
    /// Held distinct from `duration_normal` so apps that want a slower
    /// disclosure feel can adjust it without slowing every other motion.
    #[serde(with = "duration_millis")]
    pub duration_collapse: Duration,
    /// ~900 ms — period of one full sweep for indeterminate progress
    /// bars and the future spinner. Long enough to read as continuous
    /// motion without strobing.
    #[serde(with = "duration_millis")]
    pub duration_indeterminate_sweep: Duration,
    /// ~200 ms — hover dwell before a plain or rich tooltip shows.
    #[serde(with = "duration_millis", default = "default_tooltip_delay")]
    pub tooltip_delay: Duration,
    /// ~400 ms — hover dwell for heavyweight tooltips (composite surfaces,
    /// scene-item tips). Longer so heavy content doesn't pop on transient
    /// hover.
    #[serde(with = "duration_millis", default = "default_tooltip_delay_heavy")]
    pub tooltip_delay_heavy: Duration,
    /// Standard easing curve. Int UI uses one mild ease-out everywhere.
    pub easing_standard: Easing,
}

/// Serde default for [`MotionTokens::tooltip_delay`] — keeps themes
/// serialized before this field deserializable.
fn default_tooltip_delay() -> Duration {
    Duration::from_millis(200)
}

/// Serde default for [`MotionTokens::tooltip_delay_heavy`].
fn default_tooltip_delay_heavy() -> Duration {
    Duration::from_millis(400)
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            duration_instant: Duration::from_millis(0),
            duration_fast: Duration::from_millis(120),
            duration_normal: Duration::from_millis(200),
            duration_slow: Duration::from_millis(300),
            duration_collapse: Duration::from_millis(200),
            duration_indeterminate_sweep: Duration::from_millis(900),
            tooltip_delay: Duration::from_millis(200),
            tooltip_delay_heavy: Duration::from_millis(400),
            easing_standard: Easing::EaseOut,
        }
    }
}

mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        duration.as_millis().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_default_durations_increasing() {
        let m = MotionTokens::default();
        assert!(m.duration_instant < m.duration_fast);
        assert!(m.duration_fast < m.duration_normal);
        assert!(m.duration_normal < m.duration_slow);
        assert!(m.tooltip_delay <= m.tooltip_delay_heavy);
    }

    #[test]
    fn easing_boundaries() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert!((easing.apply(0.0) - 0.0).abs() < 0.001, "{:?} at 0", easing);
            assert!((easing.apply(1.0) - 1.0).abs() < 0.001, "{:?} at 1", easing);
        }
    }

    #[test]
    fn easing_monotonic() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            let mut prev = 0.0;
            for i in 0..=100 {
                let t = i as f32 / 100.0;
                let v = easing.apply(t);
                assert!(v >= prev - 0.001, "{:?} not monotonic at t={}", easing, t);
                prev = v;
            }
        }
    }

    #[test]
    fn lerp_basic() {
        assert_eq!(super::lerp(0.0, 100.0, 0.0), 0.0);
        assert_eq!(super::lerp(0.0, 100.0, 1.0), 100.0);
        assert_eq!(super::lerp(0.0, 100.0, 0.5), 50.0);
        assert_eq!(super::lerp(10.0, 20.0, 0.25), 12.5);
    }

    #[test]
    fn easing_clamps_input() {
        assert!((Easing::Linear.apply(-1.0) - 0.0).abs() < 0.001);
        assert!((Easing::Linear.apply(2.0) - 1.0).abs() < 0.001);
    }
}
