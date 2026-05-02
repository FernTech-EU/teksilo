//! Animation wrapper widgets — subtree wrappers that drive a
//! framework `Signal<f32>` to animate a paint-time property
//! (opacity, transform, layout slot) on the wrapped child.
//!
//! These wrappers all reuse the framework's animation infrastructure
//! (`Signal<f32>::animate_to`, `BuildContext::animate()`,
//! `MotionTokens` durations, `prefers-reduced-motion`) — they do not
//! schedule timers themselves.
//!
//! - [`Fade`] — opacity 0 ↔ 1, layout-transparent.
//! - [`Collapse`] — height 0 ↔ natural, layout-driving.
//!
//! Spinner is a separate concern: it is a *leaf* widget (shader-driven
//! `AnimatedQuadKind::SpinnerArc`), not a subtree wrapper, and lives at
//! the crate root.

pub mod blur;
pub mod collapse;
pub mod crossfade;
pub mod cycle;
pub mod fade;
pub mod pulse;
pub mod rotate;
pub mod scale;
pub mod shake;
pub mod slide;
pub mod smooth_size;

pub use blur::Blur;
pub use collapse::Collapse;
pub use crossfade::Crossfade;
pub use cycle::Cycle;
pub use fade::Fade;
pub use pulse::Pulse;
pub use rotate::Rotate;
pub use scale::{Scale, ScaleOrigin};
pub use shake::Shake;
pub use slide::{Slide, SlideEdge};
pub use smooth_size::{SmoothSize, SmoothSizeAxes};
