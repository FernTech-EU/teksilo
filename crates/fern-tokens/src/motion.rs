use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Easing curve for animations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
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

/// Motion tokens: durations and easing curves for animations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTokens {
    #[serde(with = "duration_millis")]
    pub duration_fast: Duration,
    #[serde(with = "duration_millis")]
    pub duration_normal: Duration,
    #[serde(with = "duration_millis")]
    pub duration_slow: Duration,
    pub easing_standard: Easing,
    pub easing_decelerate: Easing,
    pub easing_accelerate: Easing,
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            duration_fast: Duration::from_millis(100),
            duration_normal: Duration::from_millis(200),
            duration_slow: Duration::from_millis(400),
            easing_standard: Easing::EaseInOut,
            easing_decelerate: Easing::EaseOut,
            easing_accelerate: Easing::EaseIn,
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
        assert!(m.duration_fast < m.duration_normal);
        assert!(m.duration_normal < m.duration_slow);
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
