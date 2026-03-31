use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Easing curve for animations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Default for Easing {
    fn default() -> Self {
        Self::EaseInOut
    }
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
}
