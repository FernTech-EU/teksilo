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
| teksilo-platform, teksilo-app | The window wake target, the hidden-window gate and `pre_present_notify`, `capture_offscreen` through `render_capture`, the reclaim poll, display-refresh tracking, the automation bridge's live-image half. |
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
| AC6 | Met offscreen | The 60 Hz table under Measurements. Not yet measured in a window under `Fifo`. |
| AC7, AC8 | Not measured | Need per-thread `/proc` sampling of Miragem's workload on the release host. |
| AC9 | Bookkeeping met | `LiveTextureStats::bytes` matches the formula in the D tests, mip levels included. GTT drift over 300 s is not measured. |
| AC10 | Met | Mirror and GPU tests (D.5, D.6), on lavapipe in CI. |
| AC11 | Met; GTT not measured | Claim (r): the attachment detaches and the reclaim poll frees the textures without a frame. |
| AC12 | Not measured | Needs a real window: the offscreen figure is the phase difference. |
| AC13 | Met offscreen on RADV | The 60 Hz table under Measurements. |
| AC14 | Not measured in a window | Offscreen: 1–2 % in phase, 0 half a period apart. |
| AC15 | Met | `live_image_cost.rs`; the figures under Measurements. |
| AC16 | Met on KWin; Windows and macOS by hand | PR-2's F.5; F.3's X11 run for screenshots of a hidden window. |
| AC17 | Met | Headless pause tests (B.14, B.15, C.18–C.22), D.20, D.21. |
| AC18 | Met with the PR-3 deviation | J.1–J.5; one private `WindowWake` per burst. |
| AC19 | Met | J.6–J.8, the Avatar cache test, D.27; a picture a culling parent parks frees its texture at the next frame (`c15_a_culled_picture_frees_its_texture_at_the_next_frame`). |
| AC20 | Met | The executor tests, I.6–I.9, F.3 at scale 1.0 (Wayland) and 1.5 (X11). |

## 12. Open questions

- **Timings outside the renderer.** `Renderer::live_texture_timings` cannot be
  reached from an app on teksilo-app, which never holds its window's
  renderer. The automation bridge, the one other channel, exists only in
  debug builds, where timings mean little. Forwarding the feature through
  teksilo-app and printing the timings in the idle trace would reach a
  release build; it is new surface and has not been added.
- **Aiming at a hidden pixel.** `source` aiming refuses a pixel the fit crops
  out, as specified, but not one an ancestor's box or clip hides. Such a press
  reaches whatever is under it.
