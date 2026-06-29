<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FlushError

Debounced atomic file writer.

`DebouncedWriter` accepts new payloads via `schedule`,
batches rapid bursts inside a debounce window, and writes the most-recent
payload to disk atomically (write-temp + rename via `tempfile`).

## Single shared I/O thread

All `DebouncedWriter`s in a process share **one** background I/O
thread (lazily started on first use). Each writer registers under
a unique `WriterId` and routes its payloads through a single
`mpsc::Sender<PoolMsg>`. The shared thread:

* keeps a per-id `(deadline, payload)` pending slot,
* blocks on the next-due deadline (or waits for a message if no
  payload is pending),
* coalesces rapid `Schedule` bursts: a new payload during the
  debounce window replaces the prior one, and the deadline is
  reset on each push so debouncing matches the user's expectation
  for "wait until the burst stops, then write."

Application logic stays single-threaded — `SettingsStore` and
friends never block on I/O. `Drop` on a `DebouncedWriter` sends
an `Unregister` message that synchronously flushes the writer's
pending payload before returning, so end-of-process state is
never lost.

## Why one thread, not one-per-writer

An app that opens the K/V store + recents + window state already
has 3 writers; a richer app might have 5–10. Each idle ~99% of the
time. One shared worker is leaner and has identical semantics from
the caller's point of view.

## Example

```ignore
use bastyde_settings::flush::DebouncedWriter;
use std::time::Duration;

// Duration::ZERO: every schedule() lands on the worker's next tick.
let writer = DebouncedWriter::new(path.clone(), Duration::ZERO);
writer.schedule("key = \"value\"\n".into());
writer.flush_now().unwrap();   // blocks until the write completes
// Drop also flushes any pending payload synchronously.
```

## Builder methods at a glance

`schedule`, `flush_now`, `path`, `delay`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub enum FlushError`

Errors surfaced by `DebouncedWriter::flush_now`.

```rust
pub enum FlushError { /* variants */ }
```

### Variants

- **`Disconnected`** — The shared I/O worker thread has panicked or shut down; writes can no longer be delivered.
- **`Io`** — The atomic write (temp-file + rename) failed at the OS level.

## `pub struct DebouncedWriter`

Atomic, debounced single-file writer.

All writers in a process share one background I/O thread (see
module docs). Each writer is identified by an opaque `WriterId`;
dropping a writer synchronously flushes its pending payload before
returning.

```rust
pub struct DebouncedWriter { /* fields */ }
```

### Methods

#### `pub fn new(path: PathBuf, delay: Duration) -> Self`

Create a writer that will atomically write to `path`, coalescing
rapid `schedule` bursts inside `delay`.

`delay = Duration::ZERO` makes every `schedule` flush on the
worker's very next iteration — useful for tests.

#### `pub fn schedule(&self, serialized: String)`

Replace the pending payload. If a flush is already scheduled,
the new payload overwrites the prior one and the deadline is
reset to `now + delay` (debounce window restarts on activity).

#### `pub fn flush_now(&self) -> Result<(), FlushError>`

Force any pending payload to disk synchronously. Returns
`Ok(())` if there was nothing to write.

#### `pub fn path(&self) -> &Path`

The destination path this writer flushes to.

#### `pub fn delay(&self) -> Duration`

The debounce window configured at construction; `Duration::ZERO`
means every scheduled payload is written on the worker's next
iteration (useful in tests).
