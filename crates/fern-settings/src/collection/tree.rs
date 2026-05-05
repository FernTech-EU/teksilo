//! [`PersistedTreeModel<T>`] — bridge between a reactive
//! [`TreeModel<T>`](fern_data::TreeModel) and a [`SettingsFile`].
//!
//! On-disk shape is a recursive [`PersistedTreeNode<T>`] structure.
//! Loading walks the file building `NodeId`s; saving walks the model's
//! roots → leaves and reconstructs the nested form.
//!
//! ## Cost model
//!
//! Every mutation re-walks the entire tree to produce the on-disk
//! representation, irrespective of which subtree changed. The
//! debounced write coalesces rapid bursts so this work is paid at
//! most once per debounce window, not per mutation, but it scales
//! with the *total* node count, not the change.
//!
//! Targets like saved queries, custom menu hierarchies, and
//! project-side panels live well below 1k nodes — the bridge is fine
//! for those at any plausible mutation rate. For trees that grow
//! larger or mutate at high frequency, prefer SQLite (`rusqlite`)
//! with a per-node row, and treat this bridge as the wrong tool.
//! A future revision could exploit `TreeChange` payloads for
//! incremental serialization, but the bookkeeping is non-trivial and
//! the simple form is correct.

use std::path::PathBuf;
use std::time::Duration;

use fern_core::ObserverHandle;
use fern_data::TreeModel;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::file::{SettingsFile, SettingsFileError};
use crate::migration::{Migrator, Versioned};

/// Recursive on-disk shape for a [`TreeModel`] node.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedTreeNode<T> {
    pub value: T,
    #[serde(default = "Vec::new")]
    pub children: Vec<PersistedTreeNode<T>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TreeFile<T> {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "Vec::new")]
    pub roots: Vec<PersistedTreeNode<T>>,
}

fn default_version() -> u32 {
    1
}

impl<T> Default for TreeFile<T> {
    fn default() -> Self {
        Self {
            version: 1,
            roots: Vec::new(),
        }
    }
}

impl<T: 'static> Versioned for TreeFile<T> {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

/// A reactive tree whose mutations persist to a single TOML file.
pub struct PersistedTreeModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    model: TreeModel<T>,
    file: SettingsFile<TreeFile<T>>,
    _handle: ObserverHandle,
}

impl<T> PersistedTreeModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    /// Open the file at `path` (running `migrator`), seed the model
    /// from its contents, and wire up persistence on every mutation.
    pub fn open(
        path: PathBuf,
        delay: Duration,
        migrator: Migrator<TreeFile<T>>,
    ) -> Result<Self, SettingsFileError> {
        let file: SettingsFile<TreeFile<T>> = SettingsFile::load(path, delay, &migrator)?;
        let snapshot = file.snapshot();
        let model = TreeModel::new();
        for (i, root) in snapshot.roots.iter().enumerate() {
            insert_recursive(&model, None, i, root);
        }

        let model_for_obs = model.clone();
        let file_for_obs = file.clone();
        let handle = model.observe_changes(move |_change| {
            let roots = serialize_tree(&model_for_obs);
            let _ = file_for_obs.replace(TreeFile {
                version: <TreeFile<T> as Versioned>::CURRENT_VERSION,
                roots,
            });
        });

        Ok(Self {
            model,
            file,
            _handle: handle,
        })
    }

    pub fn model(&self) -> &TreeModel<T> {
        &self.model
    }

    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.file.flush_now()
    }

    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}

impl<T> std::fmt::Debug for PersistedTreeModel<T>
where
    T: Clone + Serialize + DeserializeOwned + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistedTreeModel")
            .field("path", &self.file.path())
            .field("root_count", &self.model.root_count())
            .finish()
    }
}

fn insert_recursive<T: Clone + 'static>(
    model: &TreeModel<T>,
    parent: Option<fern_data::NodeId>,
    index: usize,
    node: &PersistedTreeNode<T>,
) {
    let id = match parent {
        Some(p) => model.insert_child(p, index, node.value.clone()),
        None => model.insert_root(index, node.value.clone()),
    };
    for (i, child) in node.children.iter().enumerate() {
        insert_recursive(model, Some(id), i, child);
    }
}

fn serialize_tree<T: Clone + 'static>(model: &TreeModel<T>) -> Vec<PersistedTreeNode<T>> {
    (0..model.root_count())
        .map(|i| serialize_subtree(model, model.root(i)))
        .collect()
}

fn serialize_subtree<T: Clone + 'static>(
    model: &TreeModel<T>,
    node: fern_data::NodeId,
) -> PersistedTreeNode<T> {
    let value = model
        .with_item(node, |t| t.clone())
        .expect("serialize_subtree: live node id must resolve");
    let children = (0..model.child_count(node))
        .map(|i| serialize_subtree(model, model.child(node, i)))
        .collect();
    PersistedTreeNode { value, children }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Folder {
        name: String,
    }

    #[test]
    fn empty_tree_after_open_then_clear() {
        // After a non-empty insert + remove cycle, the persisted file
        // should reflect an empty tree.
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.toml");
        let ptm: PersistedTreeModel<Folder> =
            PersistedTreeModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();

        let n = ptm.model().insert_root(0, Folder { name: "x".into() });
        ptm.model().remove(n);
        ptm.flush_now().unwrap();

        let on_disk: TreeFile<Folder> =
            toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(on_disk.roots.is_empty());
    }

    #[test]
    fn nested_inserts_persist() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.toml");

        {
            let ptm: PersistedTreeModel<Folder> =
                PersistedTreeModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
            let m = ptm.model();
            let root = m.insert_root(
                0,
                Folder {
                    name: "root".into(),
                },
            );
            let child = m.insert_child(
                root,
                0,
                Folder {
                    name: "child".into(),
                },
            );
            m.insert_child(
                child,
                0,
                Folder {
                    name: "leaf".into(),
                },
            );
            ptm.flush_now().unwrap();
        }

        let ptm: PersistedTreeModel<Folder> =
            PersistedTreeModel::open(path, Duration::ZERO, Migrator::new()).unwrap();
        assert_eq!(ptm.model().root_count(), 1);
        let root = ptm.model().root(0);
        assert_eq!(
            ptm.model().with_item(root, |f| f.name.clone()).unwrap(),
            "root"
        );
        let children = ptm.model().children(root);
        assert_eq!(children.len(), 1);
        let child = children[0];
        let grand = ptm.model().children(child);
        assert_eq!(grand.len(), 1);
        assert_eq!(
            ptm.model().with_item(grand[0], |f| f.name.clone()).unwrap(),
            "leaf"
        );
    }

    #[test]
    fn update_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.toml");
        let ptm: PersistedTreeModel<Folder> =
            PersistedTreeModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let n = ptm.model().insert_root(0, Folder { name: "old".into() });
        ptm.model().update(n, Folder { name: "new".into() });
        ptm.flush_now().unwrap();

        let parsed: TreeFile<Folder> = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.roots[0].value.name, "new");
    }

    #[test]
    fn move_node_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.toml");
        let ptm: PersistedTreeModel<Folder> =
            PersistedTreeModel::open(path.clone(), Duration::ZERO, Migrator::new()).unwrap();
        let m = ptm.model();
        let a = m.insert_root(0, Folder { name: "a".into() });
        let _b = m.insert_root(1, Folder { name: "b".into() });
        let c = m.insert_child(a, 0, Folder { name: "c".into() });
        m.move_to_root(c, 0);
        ptm.flush_now().unwrap();

        let parsed: TreeFile<Folder> = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.roots.len(), 3);
        assert_eq!(parsed.roots[0].value.name, "c");
    }
}
