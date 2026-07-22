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
* coalesces rapid `Schedule` bursts by **appending** to the queue,
  resetting the failure streak (a just-queued patch has never itself
  failed to write) and moving the deadline **forward, never backward** —
  so debouncing collapses *writes*, never *mutations*, and a live
  `RETRY_BACKOFF` deadline installed after a failed attempt can't be
  clobbered back to "now" by an unrelated new patch on a zero-delay
  writer. (The old design could overwrite the pending payload precisely
  because each payload was a complete, self-superseding rendering.)

A failed write **retains** the queue and retries with backoff, up to
`MAX_WRITE_ATTEMPTS` — the patches replay cleanly against whatever is on
disk then, which is the correct merge rather than a stale overwrite. Once
the cap is reached (or a writer is dropped mid-failure at process
teardown), the queue is discarded for good and reported through the
process-wide `WriteFailureSink` (registered via
`set_write_failure_sink`) in addition to the existing log — the write
side's analogue of `crate::reload::Reloadable`'s read-side contract.
Conversely, every writer may also register a `WriteLandedSink` (via
`DebouncedWriter::set_landed_sink`) to learn the *real* on-disk stamp
the instant its queued patches land successfully — useful to a caller
whose own `apply()`-style API schedules a write and returns before it's
actually on disk.

The locked read-merge-write (`apply_and_write`) acquires its advisory
lock **non-blocking**: because every writer in the process shares this
one thread, a lock held by a peer process must never stall it — a
contended lock is just another transient `FlushError::Io`, retried
with the same backoff as any other write failure.

Application logic stays single-threaded — `SettingsStore` and friends never
block on I/O. `Drop` sends an `Unregister` that synchronously flushes the
queue before returning, so end-of-process state is never lost (unless the
flush itself is still failing, in which case the discard is reported
through `WriteFailureSink` exactly as above).

## Why one thread, not one-per-writer

An app that opens the K/V store + recents + window state already has 3
writers; a richer app might have 5–10, each idle ~99% of the time. One shared
worker is leaner and has identical semantics from the caller's point of view.

## Builder methods at a glance

`flush_now`, `set_landed_sink`, `path`, `delay`

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

## `pub type WriteFailureSink`

Invoked (off the caller's thread — on the shared worker thread) when a
`DebouncedWriter`'s queued patches are **permanently** discarded: either
`flush_writer` gave up after `MAX_WRITE_ATTEMPTS`, or the writer was
dropped (`Unregister`) while its final flush was still failing. This is
the write-side analogue of `crate::reload::Reloadable`'s read-side
contract — the previous behaviour was a bare `eprintln!` that never left
the worker thread, so a permanently unwritable settings file (read-only
mount, revoked permissions, disk full) silently ate every change for the
rest of the session with zero signal to the application. Registered
process-wide via `set_write_failure_sink`.

```rust
pub type WriteFailureSink = Arc<dyn Fn(PathBuf, u32, usize, String) + Send + Sync + 'static>;
```

## `pub fn set_write_failure_sink(...)`

Register a process-wide sink invoked whenever any `DebouncedWriter`
permanently discards a queued write (see `WriteFailureSink`). There is
only one slot: a later call replaces an earlier one. `bastyde-app` uses
this to forward the failure to the UI thread as a typed `AppEvent`.

```rust
pub fn set_write_failure_sink(sink: WriteFailureSink);
```

## `pub type LandedStamp`

The `(mtime, len)` stamp `disk_stamp` computes for a settings file —
named so every `Arc<Mutex<...>>` wrapping it (here and in
`WindowStateService`) reads as one term instead of clippy's
`type_complexity`-tripping nested-generics spelling.

```rust
pub type LandedStamp = (Option<SystemTime>, Option<u64>);
```

## `pub type WriteLandedSink`

Invoked on the shared worker thread the instant a `DebouncedWriter`'s
queued patches land successfully, with the fresh on-disk `(mtime, len)`
stamp (one extra `fs::metadata`, computed once, right after the write —
negligible cost). The write-side analogue of `WriteFailureSink`. `Send +
Sync` because it runs off the caller's thread — a consumer that needs to
update `!Send` state (an `Rc<Cell<_>>`) must copy the value out on its
own thread the next time it looks (see
`WindowStateService::reload_from_disk`).

```rust
pub type WriteLandedSink = Arc<dyn Fn(LandedStamp) + Send + Sync + 'static>;
```

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

#### `pub fn set_landed_sink(&self, sink: WriteLandedSink)`

Register a sink for this writer's successful-flush stamp (opt-in; a
writer with none behaves exactly as today). May be called any time
after construction — including after the writer has already flushed
once, since the sink is only ever consulted on a *future* successful
flush.

This is how a caller learns the *real* on-disk stamp resulting from
its own debounced write, without guessing: `apply()` schedules a
patch and returns before it lands, so only the worker thread — right
after the write actually succeeds — knows the resulting `(mtime,
len)`. See `WindowStateService::reload_from_disk` for the consumer
side (F11).

#### `pub fn path(&self) -> &Path`

The destination path this writer flushes to.

#### `pub fn delay(&self) -> Duration`

The debounce window configured at construction; `Duration::ZERO`
means every scheduled payload is written on the worker's next
iteration (useful in tests).
