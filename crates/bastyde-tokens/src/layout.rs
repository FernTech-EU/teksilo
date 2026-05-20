use serde::{Deserialize, Serialize};

/// Layout tokens — the entire generic spacing surface in Int UI.
///
/// Per-component spacing lives in per-widget recipe style structs in
/// `bastyde-widgets/src/styles/`, not here. Only values that are
/// genuinely cross-cutting belong on this struct.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutTokens {
    /// Gap between sibling form controls in a row or column.
    pub control_gap: f32,
    /// Gap between sections of a panel or dialog.
    pub section_gap: f32,
}

impl Default for LayoutTokens {
    fn default() -> Self {
        Self {
            control_gap: 8.0,
            section_gap: 16.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_default_section_gap_larger_than_control_gap() {
        let l = LayoutTokens::default();
        assert!(l.control_gap < l.section_gap);
    }
}
