use fern_tokens::Color;

use crate::geometry::Point;

/// A paint type for filling shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Color),
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
    Image(ImageHandle),
}

/// Handle to an image resource.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageHandle {
    pub name: String,
}

/// A single stop in a gradient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Solid(color)
    }
}
