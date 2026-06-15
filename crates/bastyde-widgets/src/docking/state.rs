// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Serializable layout state for [`DockingModel`](super::DockingModel).
//!
//! Only **user-controllable** values are persisted (per-side size /
//! visibility / presentation / selection and the full tab → arrangement
//! tree, plus corner ownership). App-config — rail thickness, minimum sizes,
//! content factories, closable flags — is declared each run and reconstructed
//! (Qt `saveState` parity). Drops into the framework persistence layer via
//! [`bastyde_settings::Versioned`] + `SettingsFile<DockLayoutState>`.

use bastyde_settings::Versioned;
use serde::{Deserialize, Serialize};

use crate::splitter::SplitterState;

use super::geometry::CornerOwners;
use super::model::TabPresentation;

/// One tab's persisted state: its Splitter sizing + the dock id in each pane
/// (one dock per Splitter pane).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockTabState {
    pub id: u64,
    #[serde(default)]
    pub splitter: SplitterState,
    /// One dock id per Splitter pane.
    #[serde(default)]
    pub panes: Vec<u64>,
    /// User-hidden activity ("Hide" / unchecked in the activities list).
    #[serde(default)]
    pub hidden: bool,
}

/// One side's persisted state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockSideState {
    pub presentation: TabPresentation,
    pub size_px: f32,
    pub visible: bool,
    #[serde(default)]
    pub selected_tab: usize,
    #[serde(default)]
    pub tabs: Vec<DockTabState>,
    /// Activity-bar item size: `0` = configured/default, `1` = compact,
    /// `2` = icon + 90°-rotated label.
    #[serde(default)]
    pub rail_size: usize,
    /// Dock-tab display mode: `0` = text, `1` = icon, `2` = icon + text.
    #[serde(default)]
    pub tab_display: usize,
}

impl Default for DockSideState {
    fn default() -> Self {
        Self {
            presentation: TabPresentation::Strip,
            size_px: 240.0,
            visible: false,
            selected_tab: 0,
            tabs: Vec::new(),
            rail_size: 0,
            tab_display: 0,
        }
    }
}

/// The full serializable snapshot of a [`DockingModel`](super::DockingModel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockLayoutState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub leading: DockSideState,
    #[serde(default)]
    pub trailing: DockSideState,
    #[serde(default)]
    pub top: DockSideState,
    #[serde(default)]
    pub bottom: DockSideState,
    #[serde(default)]
    pub corners: CornerOwners,
}

fn default_version() -> u32 {
    DockLayoutState::CURRENT_VERSION
}

impl Default for DockLayoutState {
    fn default() -> Self {
        Self {
            version: DockLayoutState::CURRENT_VERSION,
            leading: DockSideState::default(),
            trailing: DockSideState::default(),
            top: DockSideState::default(),
            bottom: DockSideState::default(),
            corners: CornerOwners::default(),
        }
    }
}

impl Versioned for DockLayoutState {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_settings::{MigrationError, Migrator};

    /// A representative non-default snapshot exercising every pane shape.
    fn sample_state() -> DockLayoutState {
        DockLayoutState {
            version: DockLayoutState::CURRENT_VERSION,
            leading: DockSideState {
                presentation: TabPresentation::Rail,
                size_px: 280.0,
                visible: true,
                selected_tab: 1,
                tabs: vec![
                    DockTabState {
                        id: 1,
                        splitter: SplitterState::default(),
                        panes: vec![10],
                        hidden: false,
                    },
                    DockTabState {
                        id: 2,
                        splitter: SplitterState::default(),
                        // Two docks split side-by-side within one tab.
                        panes: vec![20, 21],
                        hidden: true,
                    },
                ],
                rail_size: 2,
                tab_display: 2,
            },
            trailing: DockSideState::default(),
            top: DockSideState::default(),
            bottom: DockSideState::default(),
            corners: CornerOwners::default(),
        }
    }

    #[test]
    fn toml_round_trips_through_the_migrator() {
        // The full persistence loop: serialize → migrate (no steps needed for
        // a current-version payload) → deserialize, byte-for-byte equal.
        let state = sample_state();
        let value = toml::Value::try_from(&state).expect("serialize");
        let restored = Migrator::<DockLayoutState>::new()
            .run(value)
            .expect("migrate");
        assert_eq!(restored, state);
    }

    #[test]
    fn missing_version_field_loads_as_current() {
        // A pre-versioning file omits `version`; the migrator treats it as the
        // baseline and serde stamps the current version on deserialize.
        let state = sample_state();
        let mut value = toml::Value::try_from(&state).unwrap();
        value.as_table_mut().unwrap().remove("version");
        let restored = Migrator::<DockLayoutState>::new().run(value).unwrap();
        assert_eq!(restored.version, DockLayoutState::CURRENT_VERSION);
        assert_eq!(restored.leading.tabs.len(), 2, "payload survives");
    }

    #[test]
    fn newer_than_current_is_refused() {
        // A file written by a future build must be refused, not silently
        // truncated.
        let mut value = toml::Value::try_from(&sample_state()).unwrap();
        value
            .as_table_mut()
            .unwrap()
            .insert("version".into(), toml::Value::Integer(99));
        let err = Migrator::<DockLayoutState>::new().run(value).unwrap_err();
        assert!(
            matches!(
                err,
                MigrationError::NewerThanCurrent {
                    on_disk: 99,
                    current: 1
                }
            ),
            "expected NewerThanCurrent, got {err:?}"
        );
    }

    #[test]
    fn partial_legacy_toml_fills_additive_defaults() {
        // Schema evolution adds optional fields over time; an older file with
        // only a handful of keys must still load, the rest defaulting.
        let src = r#"
            [leading]
            presentation = "Rail"
            size_px = 300.0
            visible = true
        "#;
        let value: toml::Value = toml::from_str(src).expect("parse");
        let restored = Migrator::<DockLayoutState>::new().run(value).unwrap();
        assert_eq!(restored.version, DockLayoutState::CURRENT_VERSION);
        assert!(restored.leading.visible);
        assert_eq!(restored.leading.size_px, 300.0);
        assert!(restored.leading.tabs.is_empty(), "tabs default to empty");
        assert_eq!(
            restored.trailing,
            DockSideState::default(),
            "absent sides default"
        );
    }

    #[test]
    fn migration_step_promotes_a_v0_payload() {
        // Simulate a v0 file that stored a side's size under the old key
        // `size`; a 0→1 step renames it to `size_px`. Without the step the
        // payload fails to deserialize (`size_px` is a required field), so this
        // proves the step both fires and is load-bearing.
        let mut value = toml::Value::try_from(&sample_state()).unwrap();
        {
            let t = value.as_table_mut().unwrap();
            t.insert("version".into(), toml::Value::Integer(0));
            let leading = t.get_mut("leading").unwrap().as_table_mut().unwrap();
            let size = leading.remove("size_px").unwrap();
            leading.insert("size".into(), size);
        }
        let migrator = Migrator::<DockLayoutState>::new().step(0, |mut v| {
            if let Some(leading) = v.get_mut("leading").and_then(|l| l.as_table_mut())
                && let Some(size) = leading.remove("size")
            {
                leading.insert("size_px".into(), size);
            }
            Ok(v)
        });
        let restored = migrator.run(value).expect("migrate v0→v1");
        assert_eq!(restored.version, 1);
        assert_eq!(restored.leading.size_px, 280.0);
    }
}
