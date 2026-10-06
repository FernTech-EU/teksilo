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
| teksilo-canvas `live_image` | The producer side: source, writer, sessions, write guard, `LiveImageDiffWriter` (see "Whole frames" below), `SourceLock`, the packed meta word, the damage ring and upload planner, the consumer and its wake flags. The draw side: `LiveImageQuad`, `DrawCommand::LiveImage`. The renderer's decisions: `internal::LivePass<B>`, one engine for every backend. `testing::LiveImageMirror` is `LivePass` over a CPU backend. |
| teksilo-core `off_thread` | The registry of off-thread attachments (`RepaintTrigger` and live images), the layout pre-pass that turns their flags into relayouts and status changes, `device_scale_signal`. |
| teksilo-render | `WgpuBackend` (`live_texture.rs`), `DeviceHealth`, `gpu_reclaim`, `render_capture`, the live mip pass (`live_mip.wgsl` over the full-screen pass the blur shares), the timing histograms (`live_timings.rs`). |
| teksilo-platform, teksilo-app | The window wake target, the hidden-window gate and `pre_present_notify`, `capture_offscreen` through `render_capture`, the reclaim poll, display-refresh tracking, the automation bridge's live-image half, the idle trace's live line (`live-image-timings`). |
| teksilo-widgets | `LiveImage`, `LiveImageHandle`, `LiveImageSizing`; `ImageWidget::from_raw`, masked images and `Avatar` on one-commit sources (`CommittedImage`). |
| teksilo-automation, teksilo-automation-mcp | `live_image_stats`, `live_image_map`, `source` aiming, `ScreenshotMeta::live_images`, the headless fixture; the probe harness's `teksilo_probe.live_image`. |

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
create textures outside the lock, skip the lock when nothing is new, lock with
the bounds above, then fill a staged texture or write the plan's rects. Every
source no quad drew loses its textures. The rules are numbered in `LivePass`'s
own documentation, and the mirror and the GPU renderer run that same code
(the differential tests compare them).

- **No torn frame.** A whole-frame upload above the staging budget (128 MiB)
  fills a second texture over several presented frames while the previous
  picture keeps drawing. The render that completes it writes, under the same
  lock, the damage since its first band, then swaps.
- **Bands.** A write is cut into bands of at most 8 MiB of staging, so a
  failed staging allocation inside `write_texture` cannot take the device
  down with a single large copy.
- **Texture creation.** Creation runs in an out-of-memory error scope. A
  refusal draws the background and retries after a second or at another
  size. Views and bind groups are made only once the texture exists.
- **A broken source.** Every row that cannot upload (oversize, out of memory,
  device lost) parks the pixel flag, so it costs no redraw per commit.
- **Two painted sizes.** A widget attached across a resize can leave two
  painted sizes in one frame. The pass then chooses deterministically: the
  size the source has now, else the larger.

## 4. Texture lifecycle

- One texture per (window, source), shared by every widget of the window that
  shows the source.
- A texture goes at the first frame that does not draw its source. A texture
  of at most 4 MiB, from a source that committed once, moves to a parked pool
  of 16 MiB instead. The pool holds only `Weak` references, gets no wakes and
  is not observed, and an evicted texture is freed. `set_park_budget(0)`
  restores the strict rule for tests. Stats report `bytes` and `bytes_parked`
  apart.
- Dropping a texture releases it to wgpu. The device frees it only once the
  submissions that used it complete. `gpu_reclaim` records each renderer's
  last submission and polls it with a zero timeout. teksilo-app's control
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

PR-3 added the per-window wake target. Pixel wakes are dropped while the
window is hidden, and one redraw is requested when it is shown. Geometry and
relayout wakes always get through. On macOS an off-main wake is a private
`AppEvent::External` payload, intercepted first in `user_event`, never a
public variant. `exiting()` disconnects every window's waker.

## 6. Placement and input

`ImageGeometry::compute` places the source in the widget's box by fit,
alignment and orientation. `snapped` moves the picture's edges to the
device-pixel grid through the widget's effective transform times the scale
factor, and only for a translation plus an axis-aligned scale. A quad's screen
rect is the content clipped to the bounds and may be fractional; its texels
still land on pixel centres. `place_children` records the placement on the
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

## 10. Measurements

All on the reference host (AMD Radeon 890M, RADV; Mesa lavapipe for the
software rows), at the commits that introduced them. The figures are one run
each; treat them as indicative, not as limits.

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
warm-up, or 300 s for G.1 and G.2. The timing columns are the worst of the
trace's per-second lines, each over its latest 1,024 samples, in µs; the
commit-to-upload columns are in ms.

| Scenario | UI thread | `prepare` p50 / p99 | lock hold p99 | commit to upload p50 / p99 | uploads a second, least | contended |
|---|---|---|---|---|---|---|
| AC7's workload: four rects, 8 % of 720 × 1280 | 3.7 % | 100 / 245 | 231 | 3.9 / 5.6 | 59 | 0 |
| Full 1920 × 1080 frames | 7.7 % | 807 / 1,060 | 1,046 | 9.6 / 11.5 | 59 | 0 |
| Same, rotated every 5 s, in two windows | 14.0 % | 697 / 1,666 and 559 / 1,070 | 1,663 and 1,062 | 5.0 / 7.5 and 3.8 / 5.5 | 60 | 0 |
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
- **Two windows.** The window that uploads first holds the lock 1.66 ms at
  p99, against 1.06 ms for the second and 1.05–1.12 ms for one window alone,
  which puts AC13 over its goal (1.5 ms). Rotations make no difference: two
  windows with none measured 1.60 ms. It is not staging churn. The UI thread
  took 5.8 minor faults a second with two windows (one window: 0), against
  the 2,025 a frame that marked churn at 7680 × 4320. Unprivileged `perf` is
  off on this host, so this is the page-fault reading, not a profile. Two
  8 MiB copies per commit share the APU's memory with the GPU.
- **A remount** costs a texture's creation and its first upload, hence the
  churn row's p99 above 2 ms; it is not one of AC13's cases.
- **With accessibility on** (an AT-SPI client attached), AC7's workload took
  3.8 % and full 1080p frames 8.2 %, but commit-to-upload p50 rose to 12.6 ms
  and 12.7 ms. At 10 Hz each commit then costs two frames and four timer
  wakes, against one and one with accessibility off (see Open questions).

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

## 11. Acceptance criteria

| ID | State | Evidence |
|---|---|---|
| AC1 | Met | Headless: teksilo-canvas `live_image/tests.rs`, teksilo-core `live_image_tests.rs`. Live: F.3's idle check, Wayland and X11. |
| AC2 | Met off macOS | The recording-poster tests of the wake layer; claims (j) and (k) of the X11 event-loop test. macOS is not run here. |
| AC3 | Met | Headless paint counters and stats; F.3 (`paints` flat while 31 frames went by). |
| AC4, AC5 | Met | Headless, `CountingWaker`, one and two trees. |
| AC6 | Met | In a window under `Fifo`: p99 0.25 ms for AC7's workload, 1.06 ms for full 1080p (Measurements, "In a window"). Offscreen: the 60 Hz table. |
| AC7 | Met on the bench | 3.7 % of a core with AC7's workload (3.8 % with accessibility on), floor included. Miragem itself is not on Teksilo yet, so its real tree is not measured. |
| AC8 | Met | At least 59 uploads every second over 60 s; UI thread 7.7 %. |
| AC9 | Met | `LiveTextureStats::bytes` matches the formula (D tests). GPU memory drift 0.0 MiB over 300 s at 720 × 1280 and at 1080p, one texture throughout. |
| AC10 | Met | Mirror and GPU tests (D.5, D.6), on lavapipe in CI. |
| AC11 | Met | Claim (r); in a window, GPU memory back within 1 MiB 2–51 ms after each of four closes, with no growth across them. |
| AC12 | Reported | Commit to upload p50 / p99: 3.9 / 5.6 ms (AC7's workload), 9.6 / 11.5 ms (full 1080p); 12.6 / 16.3 ms with accessibility on. |
| AC13 | Not met with two windows | Lock hold p99 1.66 ms in the first of two windows; 1.05–1.12 ms with one window, rotations included. Not staging churn (page faults); see Measurements. |
| AC14 | Met | A copy-only producer: 0 contended frames of 3,609 over 60 s. |
| AC15 | Met | `live_image_cost.rs`; the figures under Measurements. |
| AC16 | Met on KWin; Windows and macOS by hand | PR-2's F.5, rerun at the tip; on the live demo, no frame over 9 s minimised, two screenshots of the hidden window 0.5 s apart holding later commits, and the latest commit shown on restore. F.3's X11 run for screenshots of a hidden window. |
| AC17 | Met | Headless pause tests (B.14, B.15, C.18–C.22), D.20, D.21. |
| AC18 | Met with the PR-3 deviation | J.1–J.5; one private `WindowWake` per burst. |
| AC19 | Met | J.6–J.8, the Avatar cache test, D.27; a picture a culling parent parks frees its texture at the next frame (`c15_a_culled_picture_frees_its_texture_at_the_next_frame`). |
| AC20 | Met | The executor tests, I.6–I.9, F.3 at scale 1.0 (Wayland) and 1.5 (X11). |

## 12. Open questions

The three left at the end of PR-5 are settled: a culling parent's parked
child now leaves the composed frame (AC19), `source` aiming refuses what a
press would not reach (section 6), and an app reads the timings in its idle
trace (section 9, commit 35). Measuring in a window found these, none of them
LiveImage's and all of them already in 0.15.1:

- **AC13 with two windows** (Measurements). The staging ring is gated on
  per-call staging allocation showing in the AC6 or AC13 profiles, and the
  page faults say it does not.
- **The accessibility delivery throttle** (teksilo-app,
  `MOVE_DELIVERY_INTERVAL`). With an AT attached, a frame within 100 ms of
  the last delivery asks for a wake 100 ms after that delivery, even when
  nothing changed since, and the wake draws a frame. An app that updates
  now and then draws each update twice for a screen-reader user.
- **The Wayland pen catch-up** (teksilo-app, `pump_pen_sources`). Every
  external wake arms a 4 ms look for the tablet shim, even on a seat that
  has announced no tablet tool, so each producer wake costs two loop
  iterations.
