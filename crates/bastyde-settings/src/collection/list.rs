// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PersistedListModel<T>`] — bridge between a reactive
//! [`ListModel<T>`](bastyde_data::ListModel) and a [`SettingsFile`].
//!
//! Construction loads the file, seeds the in-memory model from the
//! `items` field, and registers a `observe_changes` observer that
//! re-serializes the whole list on every mutation. The observer
//! captures the file via `Rc` clone — both ends are `Rc<RefCell<>>`-
//! shaped so cloning is cheap.

use std::path::PathBuf;
use std::time::Duration;

use bastyde_core::ObserverHandle;
use bastyde_data::ListModel;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::file::{SettingsFile, SettingsFileError};
use crate::migration::{Migrator, Versioned};

/// On-disk shape: a versioned wrapper around `Vec<T>`. Apps construct
/// migrations against this type, not against the bare collection.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListFile<T> {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}

fn default_version() -> u32 {
    1
}

impl<T> Default for ListFile<T> {
    fn default() -> Self {
        Self {
            version: 1,
            items: Vec::new(),
        }
    }
}

/// `T` doesn't carry a version itself; the wrapper does. This impl is
/// parameterized on the version a particular app uses by way of the
/// `Versioned for ListFile<T>` instance the app produces. We provide a
/// default `CURRENT_VERSION = 1`; apps that bump the schema replace
/// the impl via newtype.
impl<T: 'static> Versioned for ListFile<T> {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

/// A reactive list whose mutations persist to a single TOML file.
pub struct PersistedListModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    model: ListModel<T>,
    file: SettingsFile<ListFile<T>>,
    /// RAII handle for the model→file observer. Dropping it would
    /// stop persisting future mutations, so it lives as long as the
    /// `PersistedListModel`.
    _handle: ObserverHandle,
}

impl<T> PersistedListModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    /// Open the file at `path` (running `migrator`), seed the model
    /// from its contents, and wire up automatic persistence on every
    /// mutation.
    pub fn open(
        path: PathBuf,
        delay: Duration,
        migrator: Migrator<ListFile<T>>,
    ) -> Result<Self, SettingsFileError> {
        let file: SettingsFile<ListFile<T>> = SettingsFile::load(path, delay, &migrator)?;
        let snapshot = file.snapshot();
        let model = ListModel::from_vec(snapshot.items);

        let model_for_obs = model.clone();
        let file_for_obs = file.clone();
        let handle = model.observe_changes(move |_change| {
            // Re-serialize the whole list. Lists this is for (recents,
            // pinned, palettes) are <100 items — microseconds of CPU.
            let items: Vec<T> = (0..model_for_obs.len())
                .filter_map(|i| model_for_obs.with_item(i, |t| t.clone()))
                .collect();
            let _ = file_for_obs.replace(ListFile {
                version: <ListFile<T> as Versioned>::CURRENT_VERSION,
                items,
            });
        });

        Ok(Self {
            model,
            file,
            _handle: handle,
        })
    }

    /// The reactive list. Bind to `Repeater` / `ListView` directly via
    /// `model.clone()`.
    pub fn model(&self) -> &ListModel<T> {
        &self.model
    }

    /// Force any pending payload to disk synchronously.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.file.flush_now()
    }

    /// The path being written to.
    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}

impl<T> std::fmt::Debug for PersistedListModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistedListModel")
            .field("path", &self.file.path())
            .field("len", &self.model.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Item {
        name: String,
        count: i32,
    }

    #[test]
    fn fresh_file_starts_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(plm.model().len(), 0);
    }

    #[test]
    fn push_persists_and_reopens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");

        {
            let plm: PersistedListModel<Item> =
                PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
            plm.model().push(Item {
                name: "a".into(),
                count: 1,
            });
            plm.model().push(Item {
                name: "b".into(),
                count: 2,
            });
            plm.flush_now().unwrap();
        }

        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(plm.model().len(), 2);
        assert_eq!(
            plm.model().with_item(0, |x| x.clone()).unwrap(),
            Item {
                name: "a".into(),
                count: 1
            }
        );
    }

    #[test]
    fn every_mutation_variant_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();

        let m = plm.model();
        m.push(Item {
            name: "a".into(),
            count: 1,
        });
        m.push(Item {
            name: "b".into(),
            count: 2,
        });
        m.push(Item {
            name: "c".into(),
            count: 3,
        });
        m.insert(
            1,
            Item {
                name: "x".into(),
                count: 99,
            },
        );
        m.set(
            0,
            Item {
                name: "A".into(),
                count: 10,
            },
        );
        m.move_item(3, 0);
        m.remove(0);
        plm.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let parsed: ListFile<Item> = toml::from_str(&raw).unwrap();
        let names: Vec<&str> = parsed.items.iter().map(|i| i.name.as_str()).collect();
        // After: ["a"] -> ["a","b"] -> ["a","b","c"] -> ["a","x","b","c"] ->
        // ["A","x","b","c"] -> ["c","A","x","b"] -> ["A","x","b"]
        assert_eq!(names, vec!["A", "x", "b"]);
    }

    #[test]
    fn replace_all_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        plm.model().replace_all(vec![Item {
            name: "a".into(),
            count: 1,
        }]);
        plm.flush_now().unwrap();

        let parsed: ListFile<Item> = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].name, "a");
    }

    #[test]
    fn clones_share_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("list.toml");
        let plm: PersistedListModel<Item> =
            PersistedListModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let model_clone = plm.model().clone();

        model_clone.push(Item {
            name: "via-clone".into(),
            count: 1,
        });
        plm.flush_now().unwrap();

        let parsed: ListFile<Item> = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].name, "via-clone");
    }
}
