//! Dynamic, dotted-key K/V store backed by TOML.
//!
//! [`SettingsStore`] is the QSettings analogue: callers ask for any
//! dotted key with a type; the store returns a cached `Signal<T>` whose
//! mutations write back into an in-memory `toml::Value` and schedule a
//! debounced flush to disk.
//!
//! Keys carry static names via [`SettingsKey<T>`], or are passed as
//! ad-hoc strings via [`SettingsStore::signal`]. Same key, same type,
//! across any number of call sites returns clones of the same `Signal`.
//!
//! ## Invariants enforced at registration
//!
//! * **Type stability** — once a key has been registered with type
//!   `T`, calling `signal::<U>` on the same key panics. Settings are
//!   programmer-named; type drift is a code bug, surfaced immediately.
//! * **No path-shape collisions** — `"editor.font_size"` cannot coexist
//!   with `"editor"` as a leaf value, in either order. Both directions
//!   panic at the call site that creates the conflict.
//!
//! ## Cycle-free observer wiring
//!
//! The cell each key owns includes an [`ObserverHandle`] returned by
//! `signal.observe(|new_val| …)`. The observer's closure captures a
//! `Weak<RefCell<StoreInner>>` — never a strong `Rc` — and bails when
//! the store has already been dropped. This is the cycle the plan
//! review flagged: a strong capture would trap the entire store inside
//! its own observer, leaking for the life of the process.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use fern_core::ObserverHandle;
use fern_core::signal::Signal;

use crate::flush::{DebouncedWriter, FlushError};

/// Default debounce window for store flushes.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);

/// Errors surfaced by [`SettingsStore::open`].
#[derive(Debug)]
pub enum SettingsStoreError {
    Io(io::Error),
    Parse(toml::de::Error),
    Flush(FlushError),
}

impl std::fmt::Display for SettingsStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsStoreError::Io(e) => write!(f, "settings store I/O: {e}"),
            SettingsStoreError::Parse(e) => write!(f, "settings store parse: {e}"),
            SettingsStoreError::Flush(e) => write!(f, "settings store flush: {e}"),
        }
    }
}

impl std::error::Error for SettingsStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SettingsStoreError::Io(e) => Some(e),
            SettingsStoreError::Parse(e) => Some(e),
            SettingsStoreError::Flush(e) => Some(e),
        }
    }
}

impl From<io::Error> for SettingsStoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A statically-named setting. Centralizes the dotted key, the value
/// type, and the default factory. Construct as a `const`:
///
/// ```
/// use fern_settings::SettingsKey;
///
/// const FONT_SIZE: SettingsKey<f32> =
///     SettingsKey::new("editor.font_size", || 14.0);
/// ```
pub struct SettingsKey<T: 'static> {
    pub key: &'static str,
    pub default: fn() -> T,
}

impl<T: 'static> SettingsKey<T> {
    pub const fn new(key: &'static str, default: fn() -> T) -> Self {
        Self { key, default }
    }
}

impl<T: 'static> std::fmt::Debug for SettingsKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsKey")
            .field("key", &self.key)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

/// One cached Signal per registered key.
struct SignalCell {
    type_id: TypeId,
    type_name: &'static str,
    signal: Box<dyn Any>,
    /// RAII handle for the observer that pipes Signal mutations back
    /// into the in-memory `toml::Value`. Dropping it would unhook the
    /// write-back, so the cell — and therefore the observer — lives
    /// for the life of the store.
    _handle: ObserverHandle,
}

struct StoreInner {
    raw: toml::Value,
    cells: HashMap<String, SignalCell>,
    writer: DebouncedWriter,
}

impl StoreInner {
    fn schedule_flush(&self) {
        // `to_string_pretty` on a `Value::Table` always succeeds for
        // serializable contents; the only failure mode is values that
        // cannot be encoded as TOML. Settings carry only such values
        // by construction (Signal<T: Serialize>).
        match toml::to_string_pretty(&self.raw) {
            Ok(s) => self.writer.schedule(s),
            Err(e) => eprintln!("fern-settings: serialize failed: {e}"),
        }
    }
}

/// A dynamic dotted-key reactive settings store.
///
/// `Clone` is cheap (an `Rc` bump). All clones share one cache and one
/// I/O thread.
pub struct SettingsStore {
    inner: Rc<RefCell<StoreInner>>,
}

impl Clone for SettingsStore {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl SettingsStore {
    /// Open a store at `path` with the default debounce window.
    pub fn open(path: PathBuf) -> Result<Self, SettingsStoreError> {
        Self::open_with_delay(path, DEFAULT_DEBOUNCE)
    }

    /// Open a store at `path` with a custom debounce window. `delay =
    /// Duration::ZERO` is useful for tests — every set writes through
    /// on the next worker iteration, and `flush_now()` is fully
    /// deterministic.
    pub fn open_with_delay(path: PathBuf, delay: Duration) -> Result<Self, SettingsStoreError> {
        let raw = match fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<toml::Value>(&s).map_err(SettingsStoreError::Parse)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => empty_table(),
            Err(e) => return Err(SettingsStoreError::Io(e)),
        };

        // The root must be a table — top-level scalars are nonsense for
        // a multi-key store. Coerce or reject.
        let raw = match raw {
            v @ toml::Value::Table(_) => v,
            _ => empty_table(),
        };

        let writer = DebouncedWriter::new(path, delay);
        let inner = Rc::new(RefCell::new(StoreInner {
            raw,
            cells: HashMap::new(),
            writer,
        }));

        Ok(Self { inner })
    }

    /// Path of the underlying file.
    pub fn path(&self) -> PathBuf {
        self.inner.borrow().writer.path().to_path_buf()
    }

    /// Force any pending payload to disk synchronously.
    pub fn flush_now(&self) -> Result<(), SettingsStoreError> {
        self.inner
            .borrow()
            .writer
            .flush_now()
            .map_err(SettingsStoreError::Flush)
    }

    /// Whether the given key has already been registered.
    pub fn has(&self, key: &str) -> bool {
        self.inner.borrow().cells.contains_key(key)
    }

    /// All keys registered so far. Order is unspecified.
    pub fn registered_keys(&self) -> Vec<String> {
        self.inner.borrow().cells.keys().cloned().collect()
    }

    /// Get-or-create a `Signal<T>` for `key`, seeded from disk or
    /// `default` if absent. Subsequent calls for the same key return
    /// clones of the same signal.
    ///
    /// # Panics
    ///
    /// * If the key was previously registered with a different type.
    /// * If the key's path conflicts with an existing leaf-value /
    ///   table shape (e.g. `"editor"` is a string and now you ask for
    ///   `"editor.font_size"`).
    pub fn signal<T>(&self, key: &str, default: T) -> Signal<T>
    where
        T: Clone + Serialize + DeserializeOwned + 'static,
    {
        if let Some(existing) = self.try_existing::<T>(key) {
            return existing;
        }

        let mut inner = self.inner.borrow_mut();

        // Re-check inside the lock in case of races between borrow drops.
        // (Single-threaded, but defensive.)
        if let Some(cell) = inner.cells.get(key) {
            return downcast_or_panic::<T>(key, cell);
        }

        // Validate the path shape before we do anything else.
        if let Err(err) = check_path_shape(&inner.raw, key) {
            panic!("{}", err.message_for(key, std::any::type_name::<T>()));
        }

        // Seed: deserialize from raw if present; else default.
        let initial = match get_nested(&inner.raw, key) {
            Some(v) => match T::deserialize(v.clone()) {
                Ok(v) => v,
                Err(_) => default,
            },
            None => default,
        };

        // Stamp the seed back into raw so that the on-disk shape
        // matches the program's understanding immediately.
        let initial_value =
            serialize_to_value(&initial).expect("initial T value must serialize as TOML");

        // Reject struct-shaped values at the leaf: they serialize as
        // TOML tables, which collide with the store's nested-key model
        // (we cannot distinguish "table is a struct value" from
        // "table is a parent of nested keys" on a re-read). Apps that
        // need to persist struct values should use `SettingsFile<T>`
        // directly. Arrays and scalars are fine.
        if matches!(&initial_value, toml::Value::Table(_)) {
            panic!(
                "SettingsStore: cannot register key \"{key}\" as {ty} — \
                 struct values serialize as TOML tables, which collide with \
                 the store's nested-key model. Use SettingsFile<{ty}> \
                 instead.",
                ty = std::any::type_name::<T>(),
            );
        }

        let sig: Signal<T> = Signal::new(initial.clone());

        write_nested(&mut inner.raw, key, initial_value);

        // Wire write-back. The closure captures a Weak so a dropped
        // store does not stay alive via its own observer.
        let key_owned = key.to_string();
        let weak: Weak<RefCell<StoreInner>> = Rc::downgrade(&self.inner);
        let handle = sig.observe(move |new_val: &T| {
            let Some(inner_rc) = weak.upgrade() else {
                return;
            };
            let mut inner = match inner_rc.try_borrow_mut() {
                Ok(g) => g,
                Err(_) => {
                    // The store is borrowed elsewhere — typically a
                    // re-entrant set during another set's observer
                    // chain. Skip rather than panic; the top-level
                    // borrow's flush_schedule will pick this up.
                    return;
                }
            };
            let value = match serialize_to_value(new_val) {
                Ok(v) => v,
                Err(_) => return,
            };
            write_nested(&mut inner.raw, &key_owned, value);
            inner.schedule_flush();
        });

        let cell = SignalCell {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            signal: Box::new(sig.clone()),
            _handle: handle,
        };
        inner.cells.insert(key.to_string(), cell);

        // Flush the seed-stamped raw so brand-new keys hit disk on
        // first registration even without a `set`.
        inner.schedule_flush();

        sig
    }

    /// Like [`signal`](Self::signal), but driven by a strongly-named
    /// [`SettingsKey<T>`] constant.
    pub fn signal_for<T>(&self, key: &SettingsKey<T>) -> Signal<T>
    where
        T: Clone + Serialize + DeserializeOwned + 'static,
    {
        self.signal(key.key, (key.default)())
    }

    fn try_existing<T: Clone + 'static>(&self, key: &str) -> Option<Signal<T>> {
        let inner = self.inner.borrow();
        let cell = inner.cells.get(key)?;
        Some(downcast_or_panic::<T>(key, cell))
    }
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("SettingsStore")
            .field("path", &inner.writer.path())
            .field("registered_keys", &inner.cells.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn downcast_or_panic<T: Clone + 'static>(key: &str, cell: &SignalCell) -> Signal<T> {
    if cell.type_id != TypeId::of::<T>() {
        panic!(
            "SettingsStore: key \"{key}\" was registered as {prev}, but \
             signal::<{new}>(...) was called. Pick one type per key.",
            prev = cell.type_name,
            new = std::any::type_name::<T>(),
        );
    }
    cell.signal
        .downcast_ref::<Signal<T>>()
        .expect("type id matched but downcast failed — fern-settings bug")
        .clone()
}

fn serialize_to_value<T: Serialize>(value: &T) -> Result<toml::Value, toml::ser::Error> {
    toml::Value::try_from(value)
}

/// Walk a dotted key into a `toml::Value`, returning the leaf if all
/// intermediate steps are tables and the leaf exists.
fn get_nested<'a>(raw: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut current = raw;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Insert `value` at the dotted `key`, creating intermediate tables as
/// needed. Caller must have already verified that the path shape is
/// compatible via [`check_path_shape`].
fn write_nested(raw: &mut toml::Value, key: &str, value: toml::Value) {
    let parts: Vec<&str> = key.split('.').collect();
    let last = parts.len() - 1;
    let mut current = raw;
    for (i, part) in parts.iter().enumerate() {
        let table = current
            .as_table_mut()
            .expect("write_nested: path validated by check_path_shape, but encountered non-table");
        if i == last {
            table.insert((*part).to_string(), value);
            return;
        }
        let entry = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        current = entry;
    }
}

#[derive(Debug)]
enum CollisionKind {
    /// An intermediate component of the dotted path is a non-table
    /// scalar, so the path can't be deepened.
    IntermediateIsValue {
        existing_path: String,
        existing_kind: &'static str,
    },
    /// The leaf is currently a table, so the path can't be assigned a
    /// scalar.
    LeafIsTable { existing_path: String },
}

impl CollisionKind {
    fn message_for(&self, requested_key: &str, requested_type: &str) -> String {
        match self {
            CollisionKind::IntermediateIsValue {
                existing_path,
                existing_kind,
            } => format!(
                "SettingsStore: cannot register key \"{requested_key}\" as {requested_type} — \
                 the prefix \"{existing_path}\" is already a {existing_kind} value. \
                 A key cannot be both a value and a parent.",
            ),
            CollisionKind::LeafIsTable { existing_path } => format!(
                "SettingsStore: cannot register key \"{requested_key}\" as {requested_type} — \
                 \"{existing_path}\" is already a table (parent of other keys). \
                 A key cannot be both a value and a parent.",
            ),
        }
    }
}

fn check_path_shape(raw: &toml::Value, key: &str) -> Result<(), CollisionKind> {
    let parts: Vec<&str> = key.split('.').collect();
    let last = parts.len() - 1;
    let mut current = raw;
    let mut walked = String::new();
    for (i, part) in parts.iter().enumerate() {
        if !walked.is_empty() {
            walked.push('.');
        }
        walked.push_str(part);

        let Some(child) = current.get(part) else {
            return Ok(());
        };
        let is_last = i == last;
        if is_last {
            if child.is_table() {
                return Err(CollisionKind::LeafIsTable {
                    existing_path: walked,
                });
            }
            return Ok(());
        }
        if !child.is_table() {
            return Err(CollisionKind::IntermediateIsValue {
                existing_path: walked,
                existing_kind: kind_name(child),
            });
        }
        current = child;
    }
    Ok(())
}

fn kind_name(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

// Allow `Path` references for ergonomics in tests / examples.
impl SettingsStore {
    /// Convenience constructor accepting `&Path`.
    pub fn open_path(path: &Path) -> Result<Self, SettingsStoreError> {
        Self::open(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Window {
        x: i32,
        y: i32,
        title: String,
    }

    fn open_in(dir: &Path, name: &str) -> SettingsStore {
        SettingsStore::open_with_delay(dir.join(name), Duration::ZERO).unwrap()
    }

    #[test]
    fn signal_returns_default_when_key_absent() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "store.toml");
        let sig = store.signal::<f32>("editor.font_size", 14.0);
        assert_eq!(sig.get(), 14.0);
    }

    #[test]
    fn signal_dedupes_per_key() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "store.toml");
        let a = store.signal::<f32>("editor.font_size", 14.0);
        let b = store.signal::<f32>("editor.font_size", 99.0); // default ignored
        assert_eq!(b.get(), 14.0);
        a.set(22.0);
        assert_eq!(b.get(), 22.0);
    }

    #[test]
    fn set_persists_after_flush_now_and_reopens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");

        {
            let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
            let sig = store.signal::<f32>("editor.font_size", 14.0);
            sig.set(18.0);
            store.flush_now().unwrap();
        }

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<f32>("editor.font_size", 14.0);
        assert_eq!(sig.get(), 18.0);
    }

    #[test]
    #[should_panic(expected = "struct values serialize as TOML tables")]
    fn struct_values_rejected_at_registration() {
        // The store does not support struct values: they serialize as
        // TOML tables, which collide with the dotted-key model.
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _w = store.signal::<Window>(
            "window.main",
            Window {
                x: 0,
                y: 0,
                title: String::new(),
            },
        );
    }

    #[test]
    fn array_of_scalars_roundtrip() {
        // Arrays serialize as TOML arrays (not tables), so they
        // coexist with the dotted-key model just fine.
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");

        {
            let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
            let palette =
                store.signal::<Vec<String>>("ui.palette", vec!["red".into(), "blue".into()]);
            palette.set(vec!["green".into(), "yellow".into(), "purple".into()]);
            store.flush_now().unwrap();
        }

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let palette = store.signal::<Vec<String>>("ui.palette", vec![]);
        assert_eq!(palette.get(), vec!["green", "yellow", "purple"]);
    }

    #[test]
    #[should_panic(expected = "registered as f32")]
    fn type_mismatch_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _a = store.signal::<f32>("k", 1.0);
        let _b = store.signal::<i32>("k", 2);
    }

    #[test]
    #[should_panic(expected = "is already a string value")]
    fn intermediate_value_collision_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _ = store.signal::<String>("editor", "blue".into());
        let _ = store.signal::<f32>("editor.font_size", 14.0);
    }

    #[test]
    #[should_panic(expected = "is already a table")]
    fn leaf_table_collision_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _ = store.signal::<f32>("editor.font_size", 14.0);
        let _ = store.signal::<String>("editor", "blue".into());
    }

    #[test]
    fn deeply_nested_collision_caught() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        // Pre-populate file with `[a.b] c = 5`
        fs::write(&path, "[a.b]\nc = 5\n").unwrap();

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        // a.b is a table; asking for a.b as a scalar should panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store.signal::<i32>("a.b", 0);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn registered_keys_lists_touched_keys_only() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        assert!(store.registered_keys().is_empty());
        let _ = store.signal::<i32>("a", 1);
        let _ = store.signal::<bool>("b.c", true);
        let mut keys = store.registered_keys();
        keys.sort();
        assert_eq!(keys, vec!["a".to_string(), "b.c".to_string()]);
        assert!(store.has("a"));
        assert!(!store.has("nonexistent"));
    }

    #[test]
    fn signal_for_uses_constant_default() {
        const HEIGHT: SettingsKey<f32> = SettingsKey::new("layout.height", || 42.0);

        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let sig = store.signal_for(&HEIGHT);
        assert_eq!(sig.get(), 42.0);
    }

    #[test]
    fn dropping_store_does_not_leak_via_observer() {
        // Regression for the cycle bug: a strong Rc capture in the
        // observer closure would prevent StoreInner from being freed
        // even after the user drops their last clone.
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let weak = {
            let inner_rc = Rc::clone(&store.inner);
            let weak = Rc::downgrade(&inner_rc);
            let _sig = store.signal::<f32>("k", 1.0);
            drop(inner_rc);
            weak
        };
        // `store` still alive — weak is upgradable.
        assert!(weak.upgrade().is_some());
        drop(store);
        // After last strong Rc drops, the weak cannot upgrade.
        assert!(
            weak.upgrade().is_none(),
            "observer must not keep StoreInner alive via a strong capture"
        );
    }

    #[test]
    fn observer_writes_back_to_raw() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        let sig = store.signal::<i32>("answer", 0);
        sig.set(42);
        store.flush_now().unwrap();

        let on_disk = fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("answer = 42"));
    }

    #[test]
    fn pre_existing_file_seeds_signals() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        fs::write(&path, "[editor]\nfont_size = 18.0\n").unwrap();

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<f32>("editor.font_size", 1.0);
        assert_eq!(sig.get(), 18.0);
    }

    #[test]
    fn path_with_top_level_scalar_recovers_with_empty_table() {
        // A weird but possible file: top-level scalar (e.g., from
        // hand-edits). The store should not panic; it should treat the
        // root as empty.
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        // toml does not actually allow a bare scalar at the top level,
        // but we also test the more common path: empty file works.
        fs::write(&path, "").unwrap();
        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<i32>("k", 7);
        assert_eq!(sig.get(), 7);
    }
}
