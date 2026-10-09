<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# LiveImage: design record

`LiveImage` shows a raster another thread rewrites at display rate, such as a
VM screen, a video frame or a camera preview. A commit uploads only the changed
bytes and repaints no widget. GPU memory stays flat, and textures are freed at
a known frame. This record covers four things: how the implementation is laid
out, where it departs from its specification, what was measured, and the state
of each acceptance criterion. Usage is in
[the application guide](../../docs/live-image.md).

The specification is `teksilo-liveimage-widget-spec.md` (2026-09-25),
implemented for 0.16.0 on top of 0.15.1, with the corrections of the approved
plan. Section numbers below (§6.1, AC7, D.13…) are the specification's.

## 1. Where the code is

| Layer | What it holds |
|---|---|
| teksilo-canvas `wake` | `RedrawWaker`, `WakeKind`, `WakeFlag`, `WakeGate`, `CountingWaker`: waking one window from any thread, shared with `RepaintTrigger`. |
| teksilo-canvas `image_geometry` | `ImageFit`, `ImageOrientation`, `ImageGeometry` (placement, device-pixel snapping, both mappings), `PixelRect`, `oriented_crop`. |
| teksilo-canvas `live_image` | The producer side: source, writer, sessions, write guard, `LiveImageDiffWriter` (see "Whole frames" below), `SourceLock`, the packed meta word, the damage ring and upload planner, the consumer and its wake flags. The draw side: `LiveImageQuad`, `DrawCommand::LiveImage`. The renderer's decisions: `internal::LivePass<B>`, one engine for every backend, and `internal::DeviceTextures`, the per-device table a window copies another's texture through. `testing::LiveImageMirror` is `LivePass` over a CPU backend. |
| teksilo-core `off_thread` | The registry of off-thread attachments (`RepaintTrigger` and live images), the layout pre-pass that turns their flags into relayouts and status changes, `device_scale_signal`. |
| teksilo-render | `WgpuBackend` (`live_texture.rs`), `DeviceHealth`, `gpu_reclaim`, `render_capture`, the live mip pass (`live_mip.wgsl` over the full-screen pass the blur shares), the timing histograms (`live_timings.rs`). |
| teksilo-platform, teksilo-app | The window wake target, the hidden-window gate and `pre_present_notify`, `capture_offscreen` through `render_capture`, the reclaim poll, display-refresh tracking, the automation bridge's live-image half, the idle trace's live line (`live-image-timings`). |
| teksilo-widgets | `LiveImage`, `LiveImageHandle`, `LiveImageSizing`; `ImageWidget::from_raw`, masked images and `Avatar` on one-commit sources (`CommittedImage`). |
| teksilo-automation, teksilo-automation-mcp | `live_image_stats`, `live_image_map`, `source` aiming, `ScreenshotMeta::live_images`, the headless fixture; the probe harness's `teksilo_probe.live_image`. |

**Why the wake layer and the protocol are in teksilo-canvas.** teksilo-render
consumes live pictures and depends on canvas, not on core; a `RenderFrame`
carries them (`DrawCommand::LiveImage`); and a producer must use them without
the GUI, which canvas is free of (no wgpu, no winit). So they belong at
canvas's level or below it. A review on 2026-10-07 asked for a crate of their
own, on two grounds: "canvas" names only drawing, and every edit to `wake.rs`
rebuilds canvas and its 22 dependents. The second does not hold: a crate of
their own would sit below canvas, canvas would depend on it, and the same
edit would rebuild the same crates. Measured, a comment added to `wake.rs`,
to `svg.rs` or to a widget's `button.rs` each made `cargo check --workspace`
recheck all 89 crates, in about 4 s. Two splits were weighed: the wake layer
alone (about half a day, and it buys only the name), and the producer side
of live pictures (two to three days, about 25 crate-private items made
public and hidden across the new boundary, and producers spared only
canvas's image decoders). Neither was done; canvas's crate documentation now
says what it holds and why. A later split can keep today's paths as
re-exports, so it would break no one.

## 2. The protocol

**Pull on render.** A commit publishes a generation and its damage under the
source's lock, then sets each consumer's pixel flag and wakes its window. It
never reaches the widget tree. The window's next render takes the flag, locks
the source briefly and uploads the union of the damage since that window's
texture generation.

**Two flags per consumer.** One flag is for pixels, read by the live pass.
The other is for geometry (size and status), read by the layout pre-pass. The
reader clears with `swap(false, AcqRel)` before it loads state. The producer
writes state, then `swap(true, AcqRel)`, and wakes only on a `false`. Two
read-modify-writes on one location either synchronise or the producer sees
`false` and wakes. That holds without a SeqCst argument, and the loom models
check it exactly. Repeated commits leave one wake outstanding per flag and
consumer (AC4).

**The lock.** Every acquisition of the state mutex goes through `SourceLock`,
whose unlock path hands a missed wake to each consumer that tried the lock
while it was held. Renderer reads are bounded:
- a presented frame waits about one refresh, scaled by the planned bytes;
- a capture waits up to a second;
- a busy lock defers the frame, which draws the held texture, and the
  producer's unlock wakes the window.

A guard held by the UI thread (a `spawn_local` task) reads as busy rather than
deadlocking. The guard is `!Send`. Lock order is fixed and debug-asserted:
state, then the consumer list, then the waker.

**Sessions.** `writer()` joins the current session; `writer_exclusive()`
starts a new one and revokes the old writers. When a session's last writer
drops, the pixels are freed and the source becomes `Disconnected`.

**Meta word.** Size and status are packed in an `AtomicU32` (2 + 15 + 15
bits), so layout reads them without the lock. The 15-bit sides cover
`MAX_DIMENSION` (16,384).

## 3. The live pass

`LivePass::prepare` runs before each render records anything. For each source
the frame draws, in order: take the flags, choose the wanted size, refuse what
the device cannot hold, keep a paused source's texture without locking,
create textures outside the lock, skip the lock when nothing is new, copy
another window's texture when it holds the latest commit, lock with the
bounds above, then fill a staged texture or write the plan's rects. Every
source no quad drew loses its textures. The rules are numbered in `LivePass`'s
own documentation, and the mirror and the GPU renderer run that same code
(the differential tests compare them).

- **No torn frame.** A whole-frame upload above the staging budget (128 MiB)
  fills a second texture over several presented frames while the previous
  picture keeps drawing. The render that completes it writes, under the same
  lock, the damage since its first band, then swaps.
- **Bands.** A write is cut into bands of at most 8 MiB of staging, so no
  single copy needs a large staging allocation.
- **Staging kept mapped.** Under the lock, each band is copied into a chunk
  of a `MAP_WRITE | COPY_SRC` buffer that is already mapped (`live_staging`,
  8 MiB chunks, a band at a 256-byte-aligned offset), and its
  `copy_buffer_to_texture` is recorded at the head of the frame's encoder,
  before the copies between windows and the mip passes. The chunks a frame
  wrote are unmapped before its submission and mapped again after it, which
  wgpu completes once the GPU is done with them; the pool takes them back at
  its next use, polling the device once before it makes another. A chunk
  unused for a second goes, and every chunk goes at a frame that holds no
  live texture. `write_texture`, which stages each call in a buffer it
  creates, is the fallback when no chunk can be made (finding B, section
  12).
- **Texture creation.** Creation runs in an out-of-memory error scope. A
  refusal draws the background and retries after a second or at another
  size. Views and bind groups are made only once the texture exists.
- **A broken source.** Every row that cannot upload (oversize, out of memory,
  device lost) parks the pixel flag, so it costs no redraw per commit.
- **Two painted sizes.** A widget attached across a resize can leave two
  painted sizes in one frame. The pass then chooses deterministically: the
  size the source has now, else the larger.
- **One upload per commit per device.** After each render, a pass publishes
  its current textures, each with the generation it holds, to a table its
  device carries (`DeviceTextures`, held by `DeviceHealth`, so every window
  on `SharedGpu` shares it). A window whose texture lacks the latest commit,
  which another window's texture of the same size already holds, records a
  texture-to-texture copy of level 0 at the head of its frame's encoder,
  before its mip passes, and takes no lock. It copies the whole frame, since
  knowing what changed would need the lock; on the device that costs far
  less than the copy into staging it replaces. Ordering comes from the
  queue: the other window submitted the writes that filled its texture with
  its own last frame, and the copy is in a later submission. Only windows
  rendering on one thread copy from each other, since that is what keeps
  another window from writing the texture between the copy's recording and
  its submission; every window renders on the main thread.

## 4. Texture lifecycle

- One texture per (window, source), shared by every widget of the window that
  shows the source. Windows share no texture: a window paused, resized or
  hidden keeps its own picture. They share uploads instead (section 3): the
  second window to draw a commit copies the first's texture on the device.
- A texture goes at the first frame that does not draw its source. A texture
  of at most 4 MiB, from a source that committed once, moves to a parked pool
  of 16 MiB instead. The pool holds only `Weak` references, gets no wakes and
  is not observed, and an evicted texture is freed. `set_park_budget(0)`
  restores the strict rule for tests. Stats report `bytes` and `bytes_parked`
  apart.
- Dropping a texture releases it to wgpu. The device frees it only once the
  submissions that used it complete. `gpu_reclaim` records, per device, each
  submission a renderer flags after dropping a texture (its last frame's, or
  its last one when it closes), keeps every one until it completes, and polls
  them with a zero timeout. A later flag does not replace an earlier one:
  windows sharing a device flag in the order they drop textures, not in the
  order they submitted, and a closing window's last frame can be older than
  the other window's pending one. teksilo-app's control
  flow keeps a deadline while any reclaim is pending, so a closed window's
  memory returns without another frame (AC11).
- `DeviceHealth` latches a lost device per device, not per renderer: wgpu's
  `Device` equality compares ids that collide across instances. A lost device
  stops the counters, so a producer sees its stream stall.

## 5. Windows nobody can see, and the wake layer

PR-2 made a hidden window (minimised or occluded) skip acquire, render,
present and the live pass, while it still runs idle callbacks, layout and
accessibility sync when woken. Every redraw request goes through one door,
`ManagedWindow::request_redraw`, which drops and remembers it while the
window is hidden. A hidden window, or one whose redraw is outstanding,
contributes no timer deadline. That fixed a 100 %-CPU spin. Wayland's
`pre_present_notify` is called only after a successful acquire, adjacent to
present.

On macOS a window already showing stays visible to AppKit while the
displays sleep, the screen is locked or another user's session has the
console, so wgpu acquires its frames and winit reports no occlusion. A
further input hides every window then: teksilo-platform's `SessionWatch`
observes `NSWorkspace`'s screens-did-sleep and session-did-resign-active
notifications and their opposites, and the distributed
`com.apple.screenIsLocked` and `com.apple.screenIsUnlocked`, which Apple
does not document. Those are registered for immediate delivery: Cocoa
holds the distributed centre's notifications while the application is
inactive, as it is under the lock screen. Each reason is set and cleared by
its own notifications. While any holds, the loop rereads the system every
second (the main display's sleep, the session's console and lock flags)
and clears a reason it no longer reports; it never sets one, so a wrong
reading costs frames drawn for nobody, never a window that does not draw
again. A window opened meanwhile is born hidden. Elsewhere there is no
watch: a Wayland compositor stops sending frame callbacks to a surface it
does not show.

PR-3 added the per-window wake target. Pixel wakes are dropped while the
window is hidden, and one redraw is requested when it is shown. Geometry and
relayout wakes always get through. On macOS an off-main wake is a private
`AppEvent::External` payload, intercepted first in `user_event`, never a
public variant. `exiting()` disconnects every window's waker.

## 6. Placement and input

`LiveImage` places the source in its box by fit, alignment and orientation,
fitting from the picture's natural size, the one its box is measured from:
with `device_pixels`, one source pixel per device pixel, so `ImageFit::None`
and `ScaleDown` keep a texel on a device pixel. `snapped` moves the picture's edges to the
device-pixel grid through the widget's effective transform times the scale
factor, and only for a translation plus an axis-aligned scale. A transform
scope lays nothing out, so `place_children` records on the attachment the
transform it snapped under, and the layout pre-pass lays the widget out again
when the effective transform has changed and either it or the recorded one
snaps (`ImageGeometry::snaps_under`). Between two rotations nothing is
laid out, and a picture with `pixel_snap` off records none. A quad's screen
rect is the content clipped to the bounds and may be fractional; the renderer
still lands the texels of a one-to-one quad on pixel centres, unless its
draw turned `pixel_snap` off (`LiveImage` passes its own), which an animated
position needs. `place_children` records the placement on the
attachment. That is what `LiveImageHandle::map_to_source`, `live_image_map`
and `source` aiming read, so input and automation agree with paint.

`map_to_source` floors: a point in the right half of a displayed pixel maps
to that pixel, not the next.

`source` aiming refuses a pixel a press would not reach. The fit's crop is
read from the placement. The rest is the hit test the press itself takes,
`WidgetTree::hit_test_for` for the op's pointer kind, at the pixel's window
point. That covers an ancestor's box or clip, a widget drawn over the picture
and a grip whose hit reaches further for a finger, and the refusal names the
node the press would reach.

## 7. One-commit images

`ImageWidget::from_raw`, a masked image and `Avatar`'s picture move their
pixels once, with `swap_frame`, into a `LiveImageSource` held with its writer
(`CommittedImage`). Their textures therefore follow the live lifecycle: they
go when not drawn and park if small. An `Avatar` keys its source by the
image's identity and mask, so a rebuild for a new name or presence keeps the
texture. Pixels the source refuses draw nothing. An unmasked
`ImageWidget::new(&icon)` keeps the static, shared path, keyed by
`RasterIcon::texture_key`.

## 8. Whole frames: `LiveImageDiffWriter`

A producer with no damage tracking (a VM host copying its guest's
framebuffer at each vsync, a remote desktop, a renderer that redraws
everything) hands over whole frames. Through the plain writer, each one is a
full commit, a wake and a full upload, even when nothing moved.
`LiveImageDiffWriter` wraps a writer, keeps a copy of the last frame it
wrote, and commits only the rows that changed. It is not in the
specification: it was proposed after PR-5 and built on request.

- **No lock for an identical frame.** The comparison runs on the producer's
  thread against the copy. An identical frame returns `None` without touching
  the source once two lock-free reads agree that the source still holds the
  copy: the published generation, and the meta word (status `Live`, same
  size). `clear()` does not move the generation, so the meta check is what
  catches it, and a revoked session too, whose free leaves `Waiting`.
- **Exact under the lock.** A frame that changed is written under the lock
  only once the generation is still this writer's and nothing is pending.
  `wrote` covers a mark, and `force_full` covers a resize, a free and a panic
  inside a transaction; both stay set until a commit, which moves the
  generation. Otherwise the frame is written whole. A `debug_assert` holds
  the argument that the locked path always commits.
- **Rects.** Each run of consecutive changed rows becomes one rect, as wide
  as the union of its rows' changed spans. The spans are found with 64-byte
  `memcmp` chunks, then a scan of one chunk. Past 16 rects, the neighbours
  whose union adds the fewest pixels merge, so a commit keeps its rects
  inside the damage ring's capacity instead of widening to their bounding
  box.
- **The blind spot.** Another writer's `pixels_mut` write, unmarked, in a
  transaction that ended without a commit or a panic, is invisible to it.
  Such pixels reach no window either, until something marks them; catching
  them would need a full compare under the lock, which is the cost the
  design avoids.

## 9. Deviations from the specification

Each item was reported at the end of its PR group. The plan's own corrections
(the flag protocol, `SourceLock`, bounded waits, the shared `LivePass` engine
and the rest of its list) are not repeated here.

| Where | Specification | As built, and why |
|---|---|---|
| PR-2 | The gate as its own commit | Squashed into the hidden-windows commit after review: the gate's CHANGELOG entry described a bug 0.15.1 did not have. |
| PR-2 | — | A redraw still undelivered after 100 ms counts as withheld and gets non-visual ticks. Without it, a window the Wayland compositor stopped showing froze idle callbacks, layout and accessibility. AccessKit handlers wake through a posted event. |
| PR-3, AC18 | A shown terminal posts 0 `AppEvent`s per burst | A relayout or a pull always posts one private `WindowWake`, and teksilo-app decides whether to draw or tick. A direct redraw request can be held forever by a Wayland compositor, which froze a terminal whose window was then minimised. `AppEvent`s seen by the app stay at 0, and LiveImage pixel wakes still post nothing (AC2). |
| PR-3 | `RepaintTrigger::request_repaint` / `request_relayout` | Adds `request_pull`, `BuildContext::on_trigger_pull` and `PullOutcome`, so a terminal not shown takes its output in without a frame, plus two doc-hidden hooks, `WidgetTree::off_thread_wants_frame` and `PlatformWindow::show_for_frame`. |
| PR-3 | — | Terminal fixes found by review: the child's writes go through a thread of their own, the PTY closes with the engine, a parse is bounded to 8 ms per update, and a synchronized update its child never ends shows after 150 ms. |
| PR-4 | `type_text` maps letters with Shift | Space, Tab, a line break, Backspace and Escape are their keys too, because that is what a keyboard sends. |
| PR-4 | — | Aiming at a synthetic node (a scene item, a text run) is refused with the owner named, rather than aiming silently in the owner's frame. No `ButtonMask::difference`: the executor clears a released button itself. |
| Commit 28 | `.handle(&h)` builder | `with_handle(&h)`: `handle()` is already the getter. `LiveImageHandle::source()` returns `Option`, `None` before the widget is built. |
| Commit 27 | Platform re-exports the reclaim poll | teksilo-app calls `teksilo_render::poll_gpu_reclaim` directly; no platform surface was needed. |
| Commits 27, 28 | D.13 with the platform commit; catalog page and overview line with the docs | D.13 landed with the widget, which it needs. The catalog page and overview line came with the widget too. |
| Commit 25 | — | A core fix the widget tests exposed, as its own commit: a destroyed subtree is taken out of the cached frame and its parent's layout. |
| Commit 30 | The mipped texture goes when the last `Trilinear` quad leaves | Kept, its chain marked stale and rebuilt on return (the plan's rule): the texture serves `Linear` quads of the same source meanwhile. |
| C.4 | "Same as C.3" (a hundred commits) with the rect planner, and one partial upload | Ten commits. The ring holds sixteen, and a texture more than sixteen generations behind is uploaded whole, so a hundred commits could never give a partial upload; the canvas tests pin that full upload at a lag of 17. |
| Commit 33 | Headless `window_hidden` always false | `wakes` is absent headless: there is no window, and a zero would read as a measured one. |
| Commit 33 | Fixture log in a label `live-image-fixture-log` | The fixture's own accessible description: an AccessKit `Label` carries its text as its value and drops any name, so it cannot be found by one. |
| Commit 33 | `crop_visible(png, meta, mapping)` | `crop_visible(png, meta, node)`, plus `capture(session, node)`: the screenshot's own `live_images` rect is the exact record in image pixels; a node-cropped image does not carry its origin, so a mapping's window rect cannot place it. |
| Commit 33 | — | The X11 event-loop test runs in CI with the `automation` feature, for claim (t), which drives the bridge's live-image route. |
| Commit 34 | A `--release` CI step for the dirty ratio | None (the plan's choice): the dirty-ratio test is `#[ignore]`d, and the counts it pins in every build are exact. |
| Commit 35 | Timings read from the `Renderer` | Also from `PlatformWindow`, and printed by the idle trace with `live-image-timings` on teksilo-app or `teksilo`: an app never holds its window's renderer, and the automation bridge, the one other channel, exists only in debug builds. |
| Commit 35 | `Percentiles { p50, p90, p99, max }` | Also `samples`: without it an empty histogram and one of zero-microsecond samples look alike. The raw durations are recorded by canvas's `LivePass` in every build and folded into histograms by teksilo-render, so no cfg spans two crates. |
| After PR-5, AC13 | Each window uploads each commit from the source | The second window to draw a commit copies the first's texture on the device, without the lock (section 3). Two windows each copying 8 MiB per commit into staging under the lock put AC13 over its goal. A texture per device would also save GPU memory, but would break the per-window pause, two windows at two sizes during a resize, and the per-window lifecycle and stats, so each window keeps its own. `LiveTextureStats::sibling_copies` counts the copies, an attachment's `uploads` counts both, and the idle trace's live line prints `copies`. D.10 now checks one upload, then one copy. |

## 10. Measurements

All on the reference host (AMD Radeon 890M, RADV; Mesa lavapipe for the
software rows), at the commits that introduced them. A figure is one run
unless it is given as a range, which names its number of runs. They are
indicative, not limits: the plan made the release host's figures reported,
not blocking, and CI measures no timing, since a shared runner or a software
rasterizer would time nothing that holds on a user's machine. What CI holds
to exact values instead (counts, bytes, pixels, decisions) is listed under
Acceptance criteria.

**Upload cost (AC15, `tests/live_image_cost.rs`).** A full live upload of
720 × 1280, against `register_image` of the same picture:

| Build | RADV | lavapipe |
|---|---|---|
| debug | 1/61 to 1/81 | 1/59 |
| release | 1/86 | 1/59 |

The dirty upload (two rects, 282 KiB) against a full one: release 0.34
(RADV) and 0.19 (lavapipe); debug 0.50–0.54 (RADV) and 0.38 (lavapipe), which
is why the ratio is asserted only without debug assertions. The counts are
exact in every build: one `write_texture` per full frame, one per rect, and
nothing without a commit.

**The live pass at 60 Hz (AC6, AC13, `measure_the_live_pass_at_60_hz`).**
Offscreen, release, with the `live-image-timings` feature. The producer and
the render loop both run at 60 Hz, five seconds per workload, in phase and
half a period apart. Microseconds:

| Workload, phase | prepare p50 / p99 | lock hold p50 / p99 | contended renders |
|---|---|---|---|
| 720 × 1280, 4 rects (8 %), in phase, RADV | 96 / 291 | 95 / 296 | 3 of 301 |
| same, half a period apart, RADV | 91 / 249 | 87 / 236 | 0 |
| 1920 × 1080 full, in phase, RADV | 1,149 / 1,435 | 1,139 / 1,426 | 0 |
| same, half a period apart, RADV | 818 / 1,009 | 815 / 1,001 | 0 |
| 720 × 1280, 4 rects, in phase, lavapipe | 69 / 197 | 74 / 189 | 6 of 301 |
| 1920 × 1080 full, in phase, lavapipe | 1,143 / 1,402 | 1,140 / 2,708 | 2 of 301 |

Both AC6 goals hold on RADV: p99 of 0.29 ms against 0.5 ms for AC7's
workload, and of 1.43 ms against 1.5 ms for full 1080p. So does AC13's
lock-hold goal, at 1.43 ms against 1.5 ms. Offscreen there is no acquire wait
and no extra frame after a contended one, so the commit-to-upload delay
measured there is the loops' phase difference and says nothing about AC12.
An earlier in-phase run found 37 contended renders of 301: when the two loops
wake within the producer's short write, every render meets the lock. A real
window and producer drift between these cases. AC14 needs a real window
over 60 s.

**The diff writer (`tests/live_diff_writer_cost.rs`, teksilo-canvas).** For
720 × 1280, release, the producer's side of `write_frame`, as medians of 101
calls: an identical frame 50 µs, a frame with a 64 × 64 change 59 µs, against
47 µs for a plain `LiveImageWriter::write_frame` of the same frame. In debug
builds the figures are 75 µs, 183 µs and 49 µs.

**In a window (AC6 to AC14, G.1, G.2).** `tools/live_image_measure.py` runs
`live-image-bench` (release, `live-image-timings`) once per scenario, in a
private `kwin_wayland --virtual` session at 60 Hz with AT-SPI accessibility
off. It reads the idle trace's live lines, the UI thread's `utime + stime`
from `/proc/<pid>/task/<pid>/stat`, and the process's `drm-total-gtt` plus
`drm-total-vram` from `/proc/<pid>/fdinfo`. Each scenario ran 60 s after a 10 s
warm-up, or 300 s for G.1 and G.2; AC13 with the copy, from 30 s to 70 s (see
"Two windows, one upload"). The timing columns are the worst of the trace's
per-second lines, each over its latest 1,024 samples, in µs; the
commit-to-upload columns are in ms. Full 1920 × 1080 frames in one window
(AC8) ran twice more on 2026-10-07, before and after the copy: UI thread
8.2 % and 7.2 %, `prepare` p99 1,185 and 1,056 µs, lock hold p99 1,146 and
1,042 µs, at least 59 uploads every second.

| Scenario | UI thread | `prepare` p50 / p99 | lock hold p99 | commit to upload p50 / p99 | uploads a second, least | contended |
|---|---|---|---|---|---|---|
| AC7's workload: four rects, 8 % of 720 × 1280 | 3.7 % | 100 / 245 | 231 | 3.9 / 5.6 | 59 | 0 |
| Full 1920 × 1080 frames | 7.7 % | 807 / 1,060 | 1,046 | 9.6 / 11.5 | 59 | 0 |
| Same, rotated every 5 s, in two windows, each uploading every commit | 14.0 % | 697 / 1,666 and 559 / 1,070 | 1,663 and 1,062 | 5.0 / 7.5 and 3.8 / 5.5 | 60 | 0 |
| Same, the second window to draw a commit copying the first's texture, from 30 s | 9.4 % | 836 / 1,029 and 6 / 30 | 1,021 (the window that uploads) | 7.5 / 8.8 | 59, uploaded or copied | 0 |
| Copies only: 16 rows scrolled, one strip written, 720 × 1280 | 5.5 % | 377 / 495 | 480 | 0.5 / 1.4 | 60 | 0 of 3,609 frames |
| Full 720 × 1280 frames, 300 s | 5.9 % | 377 / 583 | 577 | 2.2 / 4.1 | 59 | 0 |
| Full 1920 × 1080 frames, 300 s | 7.2 % | 876 / 1,131 | 1,124 | 13.9 / 15.7 | 59 | 0 |
| Mounted and unmounted every 0.5 s, 300 s | 2.4 % | 302 / 2,317 | 2,294 | 3.9 / 18.5 | 30, mounted half the time | 0 |

- **GPU memory.** Over 300 s, the process's GTT and VRAM did not move at
  either size (AC9 allows 19 and 32 MiB), with one texture throughout, of
  w × h × 4 bytes. A second window opened and closed four times went back to
  within 1 MiB of its level before the open 2–51 ms after each close (AC11),
  from 220.3 MiB at the first open to 220.5 at the last. One earlier run kept
  4 MiB of GTT after its single close and never released it; with four
  cycles there was no growth from one to the next, which reads as an
  allocator block kept rather than a leak. Unmounted, the churn's level
  stayed between 220.3 and 220.5 MiB over 304 cycles (G.2).
- **Two windows.** When each window uploaded every commit itself, the one
  that uploaded first held the lock 1.66 ms at p99, against 1.06 ms for the
  second and 1.05–1.12 ms for one window alone, which put AC13 over its goal
  (1.5 ms). Rotations made no difference: two windows with none measured
  1.60 ms. A `perf` profile of the UI thread (DWARF call graphs, 20 s of full
  1080p frames, one window and then two) put 94 % of `write_texture`'s
  samples in `memmove`, the frame's copy into wgpu's staging buffer under the
  lock, and at most 0.6 % in allocation. The UI thread took 5.8 minor faults
  a second with two windows (one window: 0), against the 2,025 a frame that
  marked staging churn at 7680 × 4320. So it was the copy itself: each window
  copied the 8 MiB frame per commit, on an APU whose GPU shares the same
  memory.
- **Two windows, one upload.** The second window to draw a commit now copies
  the first's texture on the device (section 3). Over three runs, measured
  from 30 s, the window that uploads held the lock 1.00–1.04 ms at p99, the
  one that copies took no lock and its live pass 26–30 µs at p99, and the
  UI thread took 8.9–10.1 % of a core, against 12.0–14.6 % in two runs of
  the same binary without the copy, the same afternoon. Which window uploads
  is whichever draws first after a layout, so it can change at a rotation.
  The measurement starts at 30 s and counts a window's lock holds only from
  the seconds it uploaded: a window that only copies takes no lock, and the
  trace's latest 1,024 holds then reach back to its last upload, a rotation's
  first upload or the start-up's. GPU memory did not move (one texture per
  window). A second window opened and closed four times went back to within
  1 MiB of its level 7–47 ms after three of the closes; after the fourth,
  4 MiB stayed and did not grow, the allocator block that the same run
  without the copy also kept, at another close.
- **A remount** costs a texture's creation and its first upload, hence the
  churn row's p99 above 2 ms; it is not one of AC13's cases.
- **With accessibility on** (an AT-SPI client attached), AC7's workload took
  3.8 % and full 1080p frames 8.2 %. At 10 Hz each commit cost two frames
  and four timer wakes, against one frame and one wake with accessibility
  off: the accessibility delivery asked for a frame after any frame drawn
  within 100 ms of its last update, even with nothing to deliver, and that
  frame handed the adapter a copy of the tree it held. It now delivers, and
  holds back, only what the adapter lacks: one frame per commit,
  accessibility on or off.
- **On Wayland** each external wake armed a 4 ms look for the tablet shim's
  packets, even on a seat with no tablet tool, where none can arrive: a
  second loop turn per wake. The look is now armed only once a tool is
  announced. With both fixes, a commit costs one frame and no timer wake,
  accessibility on or off; AC7's workload took 3.2 % (off) and 2.4 % (on),
  single runs within the spread between runs.
- **Commit to upload** depends on where the producer's 60 Hz clock falls
  against the display's, which differs from one run to the next: its p50
  ranged from 2.2 to 15.1 ms across these runs and the reruns of
  2026-10-07, whatever the scenario.

**Hidden windows (AC16, PR-2, private `kwin_wayland --virtual`).** Minimised,
the window drew no frame and used 0.01 s of CPU from 0.5 s to 5 s, and it drew
again on restore. Without `pre_present_notify`, the same window rendered 31–32
frames a second for about 1 s of CPU, the specification's baseline. It also
redrew after each of 50 minimise–restore cycles and after each of 200
resizes (F.7).

**End to end (F.3, `example_live_image.py` on `live-image-demo`, debug).** On
the private Wayland session at scale 1.0, all checks passed:
- two screenshots 0.5 s apart held generations 44 and 88, neither deferred,
  with different pixels;
- the producer committed 31 frames in 0.5 s, the window showed all 31, and
  the widget's paint count stayed at 1;
- presses at the four corners and the centre reached the guest at those
  pixels, in portrait and in landscape;
- two fingers arrived as two contacts, and each cancel released one;
- keys reached the guest, Ctrl+C included, and Ctrl+Tab left the picture;
- each rotation relayouted the box;
- with the producer paused, the idle trace printed no line for 10 s, having
  printed while frames flowed.

With the demo's *Whole frames* on, a commit through the diff writer carried
24.5 KB on average against a 3.7 MB frame. With *Still guest* on as well, the
producer handed over sixty identical frames a second, the generation did not
move, and the idle trace stayed silent for five seconds.

On a private rootless Xwayland at scale 1.5, every check passed too. That X
server, which has no window manager, reports its windows obscured, so the
window was hidden and drew no frame. Screenshots of it still held the latest
commits, which is AC20's last clause.

**On macOS (2026-10-08).** A Mac mini with an Apple M4, Metal, an external
3440 × 1440 display at 100 Hz and scale 1.0, at `e385a694`, run by Claude
Code on that Mac from a written checklist, with Cyril at hand for what needs
a person:
- The test suite on Metal, GPU tests failing rather than skipping without an
  adapter: 10,380 passed, none failed. The automation server's smoke tests
  passed over the Unix-socket transport.
- F.3: 48 of 48 checks, on two runs; the three other curated probes passed.
  Scale 2.0 was not exercised: the display is not Retina.
- F.4: twenty runs of full 1080p frames, a second window opened and closed
  every 1.5 s, every commit waking both windows: every run ended cleanly,
  with no commit holding the producer past 50 ms. By hand, thirteen opens
  and closes of the demo's second window and a ⌘Q while it ran were smooth.
- AC16: the minimise probe passed its six checks (no frame from 0.5 s after
  the minimise to the restore, the bridge reporting the window hidden, two
  screenshots of the hidden window at later commits, the latest commit on
  restore). Covered by another window for 14 s, the window wrote no trace
  line and dropped one wake.
- AC2: one posted event per off-main draw wake (60 for 60 frames a second),
  none reaching the app, no timer.
- With VoiceOver on: one frame per commit at 10 Hz, no timer.
- The live pass on Metal: 59 to 60 uploads a second, but `prepare` p99 of
  1,068 µs for AC7's workload and 3,563 µs for full 1080p frames, lock holds
  the same, no contended frame (one run each). Finding B below.

It found two faults, open at this point (section 12):
- **A.** A window AppKit never marks visible, one created on a sleeping
  display or a locked screen, answered about 120,000 RedrawRequested a
  second and drew nothing. wgpu's Metal backend refuses to acquire while the
  window's `occlusionState` lacks *Visible*, winit reports occlusion only
  when it changes, and a skipped frame was retried without bound unless the
  window was already known to be hidden. Each new window showed the same
  briefly, 300 to 1,000 empty RedrawRequested while it first appeared.
- **B.** wgpu stages each `write_texture` in a buffer it creates for that
  call, and on Metal that is a new `MTLBuffer`, allocated and first touched
  under the source's lock. A standalone measurement on the same Mac put a
  1080p write at 2.7 / 3.7 ms (p50 / p99) paced at 60 Hz, against 1.0 /
  1.55 ms for a copy into a buffer already mapped. Vulkan pools that memory,
  which is why the profile on the reference host found allocation at 0.6 %.

**Staging kept mapped, on the reference host.** With the pool (section 3),
in alternating runs of the same scenarios: AC7's workload, `prepare` p50 /
p99 67 / 136 µs against 105 / 213 µs, lock hold p99 126 against 206 µs, UI
thread 2.9 % against 3.4 % (one run each); full 1080p frames, `prepare` p99
1,023 and 993 µs against 1,064 and 1,062 µs, lock hold p99 1,009 and 977 µs
against 1,055 and 1,048 µs, UI thread 7.1 and 7.0 % against 7.2 and 7.1 %
(two runs each). A first version copied a band row by row, and its 1080p
p99 read 1,290 µs; a band whose rows already have the copy pitch, as a
1920-pixel row does, is now one copy, as `write_texture` makes. GPU memory
did not drift over either run.

**On macOS, after the fixes (2026-10-08, evening).** The same Mac, at
`230b9945`, from a second checklist:
- The suites of teksilo-render, teksilo-app and teksilo-platform on Metal:
  498 passed, none failed, the four staging tests and the redraw gate's
  skipped-frame tests among them.
- A: the bench's second window opened at 10 s on a display put to sleep at
  3 s, the screen locking with it, and woken at 25 s. No second counted
  more than two RedrawRequested above its frames. The window opened asleep
  drew nothing and woke the loop twice a second for its occlusion check,
  and both windows drew again within 0.6 s of the wake. The process used
  6.7 s of CPU over the 40 s run, against 9.4 s for the same run awake. In
  the close loop's ten runs, each second in which a window opened counted
  2 to 4 RedrawRequested above its frames, against 300 to 1,000 before.
- F.4: ten runs of the close loop, all clean. F.3: 48 of 48 checks. The
  minimise probe: six of six.
- B, one run each. Each trace line's timings cover its latest 1,024
  samples, so lines from 18 s on hold no start-up sample. AC7's workload:
  `prepare` p50 / p90 / p99 of 248 / 317 / 376 µs (medians over the
  lines), against 577 / 733 / 896 before the pool, and a p99 of at most
  456 µs on every line after the first. Full 1080p frames: 1,213 / 1,429 /
  1,879 µs against 2,382 / 2,909 / 3,418, and a p99 between 1,836 and
  1,903 µs once start-up had left the samples. Lock holds ran 12 to 24 µs
  under `prepare`; no frame was contended. The same evening, a standalone
  copy of a 1080p frame into a buffer already mapped, paced at 60 Hz, took
  1,159 / 1,876 µs (p50 / p99), against 1,006 / 1,554 that morning, and
  124 µs back to back: the live pass costs what that copy costs on this
  machine when paced, and the pool adds nothing to it.
- The bench's first window, already showing when the display was put to
  sleep, drew 59 to 61 frames a second through the sleep and the lock:
  wgpu acquired every frame, so AppKit's `occlusionState` kept *Visible*,
  and winit reported no occlusion. Finding C (section 12).

**On macOS, finding C's fix (2026-10-08, late evening).** The same Mac, at
`7bcf0d57`, from a third checklist:
- The suites of teksilo-platform and teksilo-app: 369 passed, none failed,
  the session watch's two tests among them.
- The display put to sleep at 3 s, the screen locking with it, the second
  window opened asleep at 10 s, the display woken at 25 s: from the first
  line after the sleep to the wake, both windows drew no frame and answered
  no RedrawRequested, the window opened asleep included, and both drew
  again in the line of the wake. The process used 4.1 s of CPU over the
  40 s run, against 6.7 s before the fix and 9.4 s awake.
  `CGDisplayIsAsleep(CGMainDisplayID())`, which the once-a-second reread
  trusts, read asleep for the whole sleep.
- The screen locked with ⌃⌘Q for 18 s, the display on: no frame from the
  first line after the lock to the unlock, and frames again in the half
  second of the unlock. The reread kept the window hidden throughout, so
  the session dictionary's lock flag holds while the screen is locked on
  macOS 26.5.
- Each window hidden this way was woken once by its producer and then not
  at all, as a hidden window's pixel flag stays set until a frame takes it.
- The close loop's ten runs, F.3's 48 checks and the minimise probe's six
  passed again. Switching to another user's session was not run.

**On Windows (2026-10-09).** Windows 11 (build 22621) in a VirtualBox VM,
whose adapter has no D3D12 driver, so wgpu ran on D3D12 WARP, the CPU
rasteriser; a 1610 × 1035 display at 60 Hz and scale 1.0. At `7bcf0d57`,
run by Claude Code on that machine from a written checklist, with Cyril at
hand for what needs a person:
- The workspace's tests, GPU tests failing rather than skipping without an
  adapter: 10,385 passed, none failed. Among them, on DX12: the three
  `d10_` tests of the copy between windows and the five `live_mips` tests.
  The automation server's smoke tests passed over the named-pipe
  transport, the bridge's with a screenshot.
- F.3: 48 of 48 checks; the three other curated probes passed (13, 18 and
  8 checks). The scale was 1.0, so a scale other than 1.0 on Windows is
  still unexercised.
- AC16: the minimise probe's six checks, and a minimise by hand: no frame
  over 11.7 s minimised, one wake dropped, the latest commit on restore.
- AC2: over 2,474 trace lines, 41 minutes of them shown, no app event, no
  posted wake and no timer: a draw wake asks winit for the redraw itself.
- F.4: ten runs of the close loop, all clean, the second window copying
  the first's texture on the device. The least commits taken in a second,
  45 to 55, dipped in the second a window opened, which WARP draws on the
  CPU.
- With Narrator on, and with a UI Automation client attached mid-run on
  purpose: one frame per commit at 10 Hz, no timer.
- The live pass on WARP: `prepare` p99 at a median of 1.2 ms a second for
  AC7's workload and 4.8 ms for full 1080p frames, lock holds alike, and
  45 and 52 uploads a second at the median, following a frame rate that
  itself fell to medians of 46 and 56. WARP draws on the CPUs of a VM, so
  this says nothing of AC6, AC8 or AC13 on Windows hardware, which remain
  unmeasured.

Before this run, WARP had failed `live_mips` d23 and d25. It shades the
texels along a scissor edge at an odd coordinate wrong, so a footprint
rebuild of the mip chain (`LoadOp::Load` with a scissor) left a row or a
column of a level transparent black, and the levels below inherited it.
The footprint scissor now grows outward to even edges, clamped to the
level (`even_scissor`), which is correct on any adapter: a texel it adds
is recomputed from current texels of the level below, so the pass writes
back the value it holds. The run also found the demo's checkboxes showing
"…" for their labels: a `MinSize` layout fault outside LiveImage.

## 11. Acceptance criteria

Two kinds of evidence stand behind these states. Exact evidence runs in CI
on every commit, or in the headless and GPU test suites a commit must pass:
counts, byte formulas, pixels and the live pass's decisions. Timing, CPU and
GPU-memory evidence comes from the reference host (Measurements); there,
"Met" says that the run, or every run of a range, was within the goal on
that host, not that the goal holds on every machine. The number of runs is
given with each such figure.

| ID | State | Evidence |
|---|---|---|
| AC1 | Met | Headless: teksilo-canvas `live_image/tests.rs`, teksilo-core `live_image_tests.rs`. Live: F.3's idle check, Wayland and X11. |
| AC2 | Met | The recording-poster tests of the wake layer; claims (j) and (k) of the X11 event-loop test. On macOS, one posted event per off-main draw wake, none reaching the app (Measurements, "On macOS"). On Windows, no posted event and no app event over 2,474 trace lines (Measurements, "On Windows"). |
| AC3 | Met | Headless paint counters and stats; F.3 (`paints` flat while 31 frames went by). |
| AC4, AC5 | Met | Headless, `CountingWaker`, one and two trees. |
| AC6 | Met on the reference host; on Metal, for AC7's workload only | In a window under `Fifo`: p99 0.25 ms for AC7's workload (one run), 1.06–1.19 ms for full 1080p (three runs) (Measurements, "In a window"). Offscreen: the 60 Hz table, one run per row. On an M4 under Metal, with staging kept mapped: at most 0.46 ms for AC7's workload on every line after the first, 1.84–1.90 ms for full 1080p (one run each; 1.07 and 3.56 ms before the pool). A copy of the frame alone, paced, takes 1.88 ms at p99 there (Measurements, "On macOS, after the fixes"). On Windows only WARP has run, which does not measure it (Measurements, "On Windows"). |
| AC7 | Met on the bench, on the reference host | 2.4–3.8 % of a core with AC7's workload over four runs, accessibility on and off, floor included. Miragem itself is not on Teksilo yet, so its real tree is not measured. |
| AC8 | Met on the reference host | At least 59 uploads every second over 60 s; UI thread 7.2–8.2 % (three runs). On Windows only WARP has run, which does not measure it. |
| AC9 | Met | Exact: `LiveTextureStats::bytes` matches the formula (D tests). On the reference host, GPU memory drift 0.0 MiB over 300 s at 720 × 1280 and at 1080p, one texture throughout (one run each). |
| AC10 | Met | Mirror and GPU tests (D.5, D.6), on lavapipe in CI. |
| AC11 | Met, but for one 4 MiB block | Exact: claim (r). In a window, four runs of four opens and closes, two before the copy and two after it: GPU memory came back to within 1 MiB 2–51 ms after 13 of the 16 closes. After the other three, one in each of three runs, with and without the copy, 4 MiB stayed, and did not grow where later cycles followed: an allocator block kept, not a texture (one is 7.9 MiB). |
| AC12 | Reported | Commit to upload p50 / p99: 3.9 / 5.6 ms (AC7's workload), 9.6 / 11.5 ms (full 1080p), one run each. The p50 ranged from 2.2 to 15.1 ms across runs, set by the phase of the producer's clock against the display's, which differs from one run to the next: a spread, not noise to average out. |
| AC13 | Met on the reference host; not on Metal | Two windows: lock hold p99 1.00–1.04 ms in the window that uploads (three runs); the other copies its texture on the device and takes no lock. One window: 1.04–1.15 ms (four runs). Each window uploading every commit gave 1.61–1.69 ms (two runs); see Measurements. On an M4 under Metal, one window: 1.82–1.88 ms (one run), with no frame contended; two windows were not run there. On Windows only WARP has run, which does not measure it. |
| AC14 | Met on the reference host | A copy-only producer: 0 contended frames of 3,609 over 60 s (one run). |
| AC15 | Met | Exact: `live_image_cost.rs` counts one `write_texture` per full frame, one per rect and none without a commit, in every build. The time ratios under Measurements are one run per build and adapter. |
| AC16 | Met on KWin, macOS and Windows | PR-2's F.5, rerun at the tip; on the live demo, no frame over 9 s minimised, two screenshots of the hidden window 0.5 s apart holding later commits, and the latest commit shown on restore. F.3's X11 run for screenshots of a hidden window. On macOS, the minimise probe's six checks, three times, no frame while covered, and none through a display sleep or a screen lock since finding C's fix (Measurements, "On macOS" and the two runs after it). On Windows, the minimise probe's six checks and a minimise by hand, under WARP (Measurements, "On Windows"). |
| AC17 | Met | Headless pause tests (B.14, B.15, C.18–C.22), D.20, D.21. |
| AC18 | Met with the PR-3 deviation | J.1–J.5; one private `WindowWake` per burst. |
| AC19 | Met | J.6–J.8, the Avatar cache test, D.27; a picture a culling parent parks frees its texture at the next frame (`c15_a_culled_picture_frees_its_texture_at_the_next_frame`). |
| AC20 | Met | The executor tests, I.6–I.9, F.3 at scale 1.0 (Wayland) and 1.5 (X11). |

## 12. Open questions

The three left at the end of PR-5 are settled: a culling parent's parked
child now leaves the composed frame (AC19), `source` aiming refuses what a
press would not reach (section 6), and an app reads the timings in its idle
trace (section 9, commit 35). Measuring in a window found three more, all
settled. Two, already in 0.15.1 and not LiveImage's, are fixed
(Measurements: the accessibility delivery, the Wayland pen look). AC13 with
two windows is met: the profile ruled out staging allocation, so the staging
ring's gate (per-call allocation showing in the AC6 or AC13 profiles) did
not trip, and the second window to draw a commit now copies the first's
texture on the device instead of copying the frame into staging under the
lock (section 3). A texture per device, which would also save the second
texture's memory, was weighed and not done (section 9).

The macOS run found two more (Measurements, "On macOS"), and its re-run a
third (Measurements, "On macOS, after the fixes"):
- **A window AppKit never marks visible spun: fixed.** A skipped frame is
  retried at once the first time, then after a wait that doubles from 16 ms
  to 250 ms (`redraw_gate`), and a rendered frame ends the series. On macOS
  a skipped frame asks AppKit for the window's `occlusionState`
  (`PlatformWindow::occluded_now`); not visible, the window is marked
  occluded and the hidden-window gate closes. winit's `Occluded(false)`
  normally reveals it; the window also asks again every 500 ms, so it cannot
  stay blank should that event not come. On the Mac, a window opened on a
  sleeping display drew nothing, and drew again within 0.6 s of the wake.
- **Metal staged each upload in a new buffer under the lock: a pool halves
  it, and 1080p stays over its goal on Metal.** The staging ring's gate
  (per-call allocation in the profile) tripped on Metal, so uploads are now
  copied into staging kept mapped (section 3). On the M4 that brought
  1080p's `prepare` p99 from 3.56 ms to 1.84–1.90 ms and put AC7's workload
  under its goal. 1080p stays over its 1.5 ms there, at what a paced copy
  of the frame costs on that machine. Only taking the copy out of the lock
  would meet it: double-buffering the source, at twice its memory, or a
  producer writing straight into mapped staging, which would put GPU
  buffers in the source. Weighed again after the re-run and not done for
  0.16.0: AC6 and AC13 stand met on the reference host and over on Metal at
  1080p, where no frame has made the producer wait. On the reference host
  the pool is at parity at 1080p and faster for AC7's workload
  (Measurements).
- **C. A window already showing drew while nothing could be seen: fixed.**
  With the displays asleep and the screen locked, the bench's first window
  drew 59 to 61 frames a second for 22 s. AppKit keeps a window that was
  showing *Visible* then, so wgpu acquires its frames and winit reports no
  occlusion, and nothing else told teksilo-app. teksilo-platform's
  `SessionWatch` now hears the displays sleep and wake, the session resign
  and become active, and the screen lock and unlock, and teksilo-app hides
  every window while any of them holds (section 5). Claim (u) of the X11
  event-loop test covers teksilo-app's side, with four mutations that each
  redden it; the observers run only on a Mac. On the Mac, both windows
  drew nothing through a display sleep and through a screen lock with the
  display on, and drew again within the line of the wake or the unlock
  (Measurements, "On macOS, finding C's fix"). Switching to another user's
  session, the watch's third reason, was not run there.

The Windows run found one more, fixed before it ran, and leaves two things
unmeasured (Measurements, "On Windows"):
- **WARP mis-shaded a footprint rebuild of the mip chain: fixed.** A
  scissor edge at an odd coordinate came out transparent black on WARP;
  the footprint scissor now grows outward to even edges.
- **Not run on Windows:** a scale other than 1.0, which the VM's display
  did not have, and AC6, AC8 and AC13 on a machine with a D3D12 driver,
  which WARP, drawing on the CPU, does not measure.
