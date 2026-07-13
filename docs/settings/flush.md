<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FlushError

Debounced, **cross-process-safe** atomic file writer.

`DebouncedWriter` accepts `Patch`es — *replayable mutations* — via
`schedule`, batches rapid bursts inside a
debounce window, and then applies the whole batch to the document **read from
disk under an exclusive advisory lock**, writing the result atomically
(write-temp + fsync + rename).

## Why a patch and not a rendered string

This writer used to carry a pre-rendered `String`: the caller serialised its
entire in-memory document and the worker blindly wrote those bytes. That is
**last-write-wins by construction** — the worker had nothing to merge *with*,
so any concurrent change a peer process made to another part of the file was
silently destroyed. (A lock alone does not fix this: it serialises the two
writes but does nothing about the stale snapshot one of them was rendered
from.)

A `Patch` instead says "given the file's current text, produce its new text",
so the merge happens against reality:

```text
  lock  ->  read current  ->  apply queued patches  ->  write atomically  ->  unlock
```

Patches are built inside this crate from *owned* snapshots of each type's
pending mutations (a `Vec<(key, value)>`, a list of ops), so they capture no
`Rc` and can cross to the worker thread. Callers never see one: they keep
writing `signal.set(v)`, `mru.add(e)`, `file.mutate(|s| ..)`.

## Single shared I/O thread

All `DebouncedWriter`s in a process share **one** background I/O thread
(lazily started on first use). Each writer registers under a unique
`WriterId`. The shared thread:

* keeps a per-id `(deadline, patch queue)`,
* blocks on the next-due deadline (or waits for a message if nothing is
  pending),
* coalesces rapid `Schedule` bursts by **appending** to the queue and
  resetting the deadline — so debouncing collapses *writes*, never
  *mutations*. (The old design could overwrite the pending payload precisely
  because each payload was a complete, self-superseding rendering.)

A failed write **retains** the queue and retries with backoff, up to
`MAX_WRITE_ATTEMPTS` — the patches replay cleanly against whatever is on
disk then, which is the correct merge rather than a stale overwrite.

Application logic stays single-threaded — `SettingsStore` and friends never
block on I/O. `Drop` sends an `Unregister` that synchronously flushes the
queue before returning, so end-of-process state is never lost.

## Why one thread, not one-per-writer

An app that opens the K/V store + recents + window state already has 3
writers; a richer app might have 5–10, each idle ~99% of the time. One shared
worker is leaner and has identical semantics from the caller's point of view.

## Builder methods at a glance

`flush_now`, `path`, `delay`

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
- **`Merge`** — A `Patch` could not be applied to the document currently on disk — e.g. a peer wrote something this process cannot parse or migrate.

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

#### `pub fn flush_now(&self) -> Result<(), FlushError>`

Force any queued patches to disk synchronously. Returns `Ok(())` if
there was nothing queued.

#### `pub fn path(&self) -> &Path`

The destination path this writer flushes to.

#### `pub fn delay(&self) -> Duration`

The debounce window configured at construction; `Duration::ZERO`
means every scheduled payload is written on the worker's next
iteration (useful in tests).
