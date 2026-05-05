//! Schema migrations for persisted files.
//!
//! Every persisted struct carries a `version: u32` (via [`Versioned`]).
//! [`Migrator<T>`] holds an ordered set of `from_version → from_version + 1`
//! transformations expressed on raw [`toml::Value`] — pre-deserialization,
//! so a v1 file that no longer matches the v2 type can still be upgraded.
//!
//! ```
//! use fern_settings::{Versioned, Migrator};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
//! struct Recents {
//!     version: u32,
//!     items: Vec<Entry>,
//! }
//!
//! #[derive(Serialize, Deserialize, Debug, PartialEq)]
//! struct Entry { path: String, pinned: bool }
//!
//! impl Versioned for Recents {
//!     const CURRENT_VERSION: u32 = 2;
//!     fn version(&self) -> u32 { self.version }
//!     fn set_version(&mut self, v: u32) { self.version = v; }
//! }
//!
//! // v1 didn't have `pinned`; supply false.
//! let migrator: Migrator<Recents> = Migrator::new()
//!     .step(1, |mut v| {
//!         if let Some(items) = v.get_mut("items").and_then(|i| i.as_array_mut()) {
//!             for item in items {
//!                 if let Some(t) = item.as_table_mut() {
//!                     t.insert("pinned".into(), toml::Value::Boolean(false));
//!                 }
//!             }
//!         }
//!         Ok(v)
//!     });
//! ```

use std::marker::PhantomData;

use serde::de::DeserializeOwned;

/// A persisted struct whose schema is versioned.
///
/// `CURRENT_VERSION` is the version this build of the code reads and
/// writes. Files on disk may be older — [`Migrator`] walks them up.
pub trait Versioned {
    /// The version this build understands. Bump when the schema changes
    /// in a way that requires migration.
    const CURRENT_VERSION: u32;

    /// The version embedded in this instance.
    fn version(&self) -> u32;

    /// Write a new version into this instance. Used by the migrator
    /// after a successful chain of steps.
    fn set_version(&mut self, v: u32);
}

/// Errors surfaced by [`Migrator::run`].
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// The on-disk version is newer than this build can read. Refuse
    /// to deserialize rather than risk silent corruption.
    #[error("settings file is version {on_disk}, but this build only reads up to {current}")]
    NewerThanCurrent { on_disk: u32, current: u32 },
    /// No migration step is registered for the on-disk version.
    #[error("no migration step registered for settings version {0}")]
    NoStepFor(u32),
    /// A migration step itself returned an error.
    #[error("migration step {from} -> {} failed: {message}", from + 1)]
    Step { from: u32, message: String },
    /// The post-migration value did not deserialize as the target type.
    #[error("post-migration deserialization: {0}")]
    Deserialize(#[source] toml::de::Error),
}

type StepFn = Box<dyn Fn(toml::Value) -> Result<toml::Value, String> + Send + Sync>;

struct Step {
    from: u32,
    func: StepFn,
}

/// Schema migration pipeline for a [`Versioned`] type.
///
/// Add `from → from + 1` steps with [`Migrator::step`]; the order in which
/// they're added does not matter — [`Migrator::run`] walks them in
/// version order.
pub struct Migrator<T: Versioned + DeserializeOwned> {
    steps: Vec<Step>,
    _marker: PhantomData<T>,
}

impl<T: Versioned + DeserializeOwned> Migrator<T> {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Register a step that promotes a value from `from` to `from + 1`.
    /// Steps may be registered in any order; [`run`](Self::run) finds
    /// the right one for the current version on demand.
    pub fn step<F>(mut self, from: u32, func: F) -> Self
    where
        F: Fn(toml::Value) -> Result<toml::Value, String> + Send + Sync + 'static,
    {
        self.steps.push(Step {
            from,
            func: Box::new(func),
        });
        self
    }

    /// Migrate `raw` from its on-disk version up to
    /// `T::CURRENT_VERSION`, then deserialize.
    ///
    /// Reads the version directly from the `version` field of the raw
    /// `toml::Value` — never deserializes-then-checks, because a v1
    /// payload typically fails to deserialize as the v2 type.
    ///
    /// Files missing the `version` field are treated as v1 (legacy).
    pub fn run(&self, mut raw: toml::Value) -> Result<T, MigrationError> {
        let target = T::CURRENT_VERSION;
        let mut current = peek_version(&raw).unwrap_or(1);

        if current > target {
            return Err(MigrationError::NewerThanCurrent {
                on_disk: current,
                current: target,
            });
        }

        while current < target {
            let step = self
                .steps
                .iter()
                .find(|s| s.from == current)
                .ok_or(MigrationError::NoStepFor(current))?;
            raw = (step.func)(raw).map_err(|message| MigrationError::Step {
                from: current,
                message,
            })?;
            current += 1;
            // Stamp the new version on the raw value so each subsequent
            // step (and the final deserialize) sees a coherent struct.
            if let Some(table) = raw.as_table_mut() {
                table.insert("version".into(), toml::Value::Integer(current as i64));
            }
        }

        T::deserialize(raw).map_err(MigrationError::Deserialize)
    }
}

impl<T: Versioned + DeserializeOwned> Default for Migrator<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Versioned + DeserializeOwned> std::fmt::Debug for Migrator<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Migrator")
            .field("step_count", &self.steps.len())
            .field("target_type", &std::any::type_name::<T>())
            .finish()
    }
}

fn peek_version(raw: &toml::Value) -> Option<u32> {
    raw.get("version")
        .and_then(|v| v.as_integer())
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
    struct V2 {
        version: u32,
        name: String,
        pinned: bool,
    }

    impl Versioned for V2 {
        const CURRENT_VERSION: u32 = 2;
        fn version(&self) -> u32 {
            self.version
        }
        fn set_version(&mut self, v: u32) {
            self.version = v;
        }
    }

    #[test]
    fn no_op_when_already_current() {
        let raw: toml::Value = toml::from_str("version = 2\nname = \"x\"\npinned = true").unwrap();
        let migrator: Migrator<V2> = Migrator::new();
        let v = migrator.run(raw).unwrap();
        assert_eq!(
            v,
            V2 {
                version: 2,
                name: "x".into(),
                pinned: true
            }
        );
    }

    #[test]
    fn applies_one_step() {
        let raw: toml::Value = toml::from_str("version = 1\nname = \"x\"").unwrap();
        let migrator: Migrator<V2> = Migrator::new().step(1, |mut v| {
            if let Some(t) = v.as_table_mut() {
                t.insert("pinned".into(), toml::Value::Boolean(false));
            }
            Ok(v)
        });
        let v = migrator.run(raw).unwrap();
        assert_eq!(
            v,
            V2 {
                version: 2,
                name: "x".into(),
                pinned: false
            }
        );
    }

    #[test]
    fn missing_version_treated_as_v1() {
        // Legacy file with no `version =` at all: assumed v1.
        let raw: toml::Value = toml::from_str("name = \"y\"").unwrap();
        let migrator: Migrator<V2> = Migrator::new().step(1, |mut v| {
            if let Some(t) = v.as_table_mut() {
                t.insert("pinned".into(), toml::Value::Boolean(true));
            }
            Ok(v)
        });
        let v = migrator.run(raw).unwrap();
        assert!(v.pinned);
        assert_eq!(v.version, 2);
    }

    #[test]
    fn newer_than_current_errors() {
        let raw: toml::Value = toml::from_str("version = 7\nname = \"x\"").unwrap();
        let migrator: Migrator<V2> = Migrator::new();
        let err = migrator.run(raw).unwrap_err();
        assert!(matches!(
            err,
            MigrationError::NewerThanCurrent {
                on_disk: 7,
                current: 2
            }
        ));
    }

    #[test]
    fn missing_step_errors() {
        let raw: toml::Value = toml::from_str("version = 1\nname = \"x\"").unwrap();
        // No step for v1 -> v2 registered.
        let migrator: Migrator<V2> = Migrator::new();
        let err = migrator.run(raw).unwrap_err();
        assert!(matches!(err, MigrationError::NoStepFor(1)));
    }

    #[test]
    fn step_failure_propagates() {
        let raw: toml::Value = toml::from_str("version = 1\nname = \"x\"").unwrap();
        let migrator: Migrator<V2> = Migrator::new().step(1, |_| Err("borked".into()));
        match migrator.run(raw).unwrap_err() {
            MigrationError::Step { from, message } => {
                assert_eq!(from, 1);
                assert_eq!(message, "borked");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn multi_step_chain_walks_in_order() {
        #[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
        struct V3 {
            version: u32,
            a: i32,
            b: i32,
            c: i32,
        }
        impl Versioned for V3 {
            const CURRENT_VERSION: u32 = 3;
            fn version(&self) -> u32 {
                self.version
            }
            fn set_version(&mut self, v: u32) {
                self.version = v;
            }
        }

        let raw: toml::Value = toml::from_str("version = 1\na = 1").unwrap();
        let migrator: Migrator<V3> = Migrator::new()
            .step(2, |mut v| {
                v.as_table_mut().unwrap().insert("c".into(), 3.into());
                Ok(v)
            })
            // intentionally registered out of order
            .step(1, |mut v| {
                v.as_table_mut().unwrap().insert("b".into(), 2.into());
                Ok(v)
            });

        let v = migrator.run(raw).unwrap();
        assert_eq!(
            v,
            V3 {
                version: 3,
                a: 1,
                b: 2,
                c: 3
            }
        );
    }
}
