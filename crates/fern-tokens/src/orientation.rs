use serde::{Deserialize, Serialize};

/// Orientation for widgets that can be horizontal or vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}
