<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Live pictures

`LiveImage` shows a picture that another thread rewrites many times a second:
a virtual machine's screen, a video frame, a camera preview, a remote desktop.
The producer thread writes pixels into a `LiveImageSource`. Each window that
shows the source uploads what changed since its last frame and draws it where
the widget last painted. The pixels never pass through the widget, the widget
tree or an event.

```rust
use teksilo::prelude::*;
use teksilo::widgets::{LiveImage, LiveImageSizing};

let screen = LiveImageSource::new(LivePixelFormat::Bgrx8);
let writer = screen.writer();
std::thread::spawn(move || loop {
    let frame = render_next_frame();                   // 720 × 1280 BGRX
    writer.write_frame(720, 1280, &frame, 720 * 4).unwrap();
});

let view = LiveImage::new(screen)
    .sizing(LiveImageSizing::Aspect)
    .alt(lit!("Virtual machine screen"));
```

Run the showcase with `cargo run -p live-image-demo`. Its 60 Hz producer
redraws a phone-shaped guest screen and takes the guest's input. A second
window can show the same source.

## What a frame costs

A commit that changes only pixels:

- runs no `paint()`, marks no widget and lays nothing out. The window replays
  its cached frame and uploads the changed bytes into its texture;
- wakes each window that shows the source once, however many commits come
  before that window's next frame. The frame uploads the union of everything
  that changed since that window's last upload;
- wakes a window nobody can see (minimised, hidden) not at all. That window
  uploads the latest commit when it is shown again.

Only a change of the source's size or status relayouts and repaints the
widget. The statuses are `Disconnected`, `Waiting` and `Live`. A producer that
stops committing costs nothing: the window draws no frame for it.

Each window holds one texture per source it shows, however many widgets show
that source. The texture goes at the first frame that does not draw the
picture: when the widget is destroyed, scrolled out, on a dormant page, or its
window closes. A small texture (4 MiB at most) of a source that committed once
waits in a 16 MiB pool instead, so a thumbnail scrolled out and back is not
uploaded again. GPU memory follows what is on screen, without leaking or
growing.

## Writing pixels

`LiveImageWriter` is `Send` and `Clone`; any number of threads may write. Two
ways to write:

- **One-shot calls**: `write_frame` (a whole frame, resizing if needed),
  `write_rect`, `resize`, `clear`, and `swap_frame`. `swap_frame` installs the
  producer's own buffer without a copy and hands the previous one back for
  reuse.
- **Transactions**: `writer.lock()` returns a `LiveImageWriteGuard`. Write
  through it with `write_rect`, `fill_rect`, `copy_within`, `rows_mut`, or
  `pixels_mut` plus `mark_dirty`, then call `commit()`. A transaction dropped
  without `commit()` publishes nothing; its changes go out with the next
  commit. Each commit is one *generation*.

Mark only what changed. The renderer uploads the marked rectangles and nothing
else: a guest that moves its cursor uploads a few kilobytes, not a frame.

The guard holds the source's lock, so keep transactions short. Decode into a
buffer of your own and copy it in with `write_rect`. A render that finds the
lock held draws the previous frame rather than wait, and the producer's unlock
wakes the window. The guard is `!Send`. To have clippy refuse a guard held
across an `.await`, configure `await-holding-invalid-types` as the workspace's
`clippy.toml` does.

`LivePixelFormat` takes RGBA, BGRA, and the alpha-less RGBX and BGRX, so a
producer never converts pixels. A source refuses a side above
`LiveImageSource::MAX_DIMENSION` (16,384) and a frame above `MAX_BYTES`
(1 GiB). A frame larger than one render's upload budget (128 MiB) fills over
several frames, while the previous picture keeps showing. A torn frame never
shows.

The source tells a producer how its frames are used:

- `is_observed()`: some window draws the source;
- `generation()` and `displayed_generation()`: what was committed, and what a
  window last presented;
- `is_displayed()`: the latest commit is on screen. A producer that renders on
  demand waits for it before rendering the next frame.

`writer_exclusive()` starts a new writer session and revokes the writers of
the old one: a stream that restarts cannot have a stale thread overwrite the
new stream. When the last writer of a session drops, the source frees its
pixels and becomes `Disconnected`. Keep a writer alive for as long as the
picture should show.

## Placing the picture

`LiveImageSizing` picks the widget's box:

- `Aspect` (the default): the largest box with the picture's aspect ratio
  inside what the parent proposes, rounded to whole device pixels;
- `Fill`: the whole proposal, with the picture letterboxed inside it;
- `Natural`: one source pixel per logical pixel. With `device_pixels(true)`,
  one per device pixel.

`width`, `height` and `size` pin the box and win over the mode. Inside the
box, `fit` (an `ImageFit`) and `alignment` place the picture,
`orientation` (an `ImageOrientation`) turns it, and `scaling` (a
`ScalingFilter`) samples it:

- `Linear`;
- `Nearest`, for pixel art and anything a test reads back by pixel;
- `Trilinear`, which samples a mip chain built on the GPU, for a picture shown
  much smaller than its source.

The picture's edges snap to the device-pixel grid, also under an ancestor's
scale; `pixel_snap(false)` turns that off.

`background` fills the letterbox. While the source is not `Live`, it fills the
whole box, and `placeholder` text is centred on it.

`pause_when_inactive(true)` stops a picture uploading while its window is
inactive. The window keeps the last frame it uploaded, and shows the latest
commit in one upload once it is active again. `dim_when_disabled(true)` dims
the picture in a disabled subtree. By default it keeps its full strength
there.

## Taking input

`LiveImage` handles no input itself. A VM screen attaches the handlers it
needs through `WidgetBuilder` and maps each position it receives to the source
pixel under it with the widget's `LiveImageHandle`:

```rust
let handle = LiveImageHandle::new();
let to_guest = handle.clone();
LiveImage::new(screen)
    .with_handle(&handle)
    .alt(lit!("Guest screen"))
    .focusable(true)
    .keyboard_capture(true)            // every key reaches on_key; Ctrl+Tab still leaves
    .multi_contact(MultiContact::All)  // every finger, not just the first
    .touch_action(TouchAction::NONE)   // no pan, no pinch: the guest has them
    .on_pointer_event(move |event, _ctx| {
        if let WidgetEvent::PointerDown { position, .. } = event
            && let Some((x, y)) = to_guest.map_to_source(*position)
        {
            send_to_guest(x, y);
        }
        EventResponse::Handled
    })
```

`map_to_source` uses the placement the last paint drew from, so a point maps
to the pixel shown under it, whatever the fit, the orientation, the snapping
and the window's scale. On the letterbox it returns `None`; `map_from_source`
goes the other way. `LiveImage::handle()` returns the same handle for a widget
built without `with_handle`, as long as it is called before a `WidgetBuilder`
method wraps the widget.

## Accessibility

The picture is one `Role::Image` node named by `alt`; a decorative picture
calls `a11y_hidden` instead. Pixels are not accessible content, and a commit
never changes the node. The node changes only with the source's status: the
placeholder is its description while it shows. If a status change should be
announced, the app announces it itself, reading `handle.status()`.

## Testing

A headless test needs no GPU.
`teksilo::canvas::live_image::testing::LiveImageMirror` runs the renderer's
live pass with textures in memory. It takes the same decisions as the GPU
renderer, from the same code, and reports what it uploaded, so a test reads
back exactly what a window would draw. For the real renderer, render into an
offscreen texture with `Renderer::render_capture` on a
`test_support::create_offscreen_renderer` and read it back with
`try_read_texture_rgba`. A capture always shows the latest commit: it uploads
through a pause, and waits up to a second for a producer that holds the
lock.

The automation bridge has three things for live pictures; see
[automation-mcp.md](automation-mcp.md):

- `live_image_stats {node}`: the source's, the attachment's and the window's
  counters;
- `live_image_map {node}`: where the picture lies and how source pixels map to
  window points;
- `source: [x, y]` on `inject_pointer`, `long_press` and touch steps, which
  presses the centre of a guest pixel.

A screenshot's `live_images` metadata says which commit each picture in the
image shows. The probe harness's `teksilo_probe.live_image` wraps all of this,
and `example_live_image.py` drives the demo end to end.

## Measuring

- `Renderer::live_texture_stats()` (also `PlatformWindow::live_texture_stats()`)
  reports what a window holds and uploads: textures and their bytes, parked
  ones apart, uploads and their bytes, `write_texture` calls, contended frames
  and lost devices.
- `Renderer::live_texture_timings()` gives percentiles of the live pass's frame
  time, of how long the renderer holds a source's lock, and of the delay from a
  commit to its upload. Debug builds always have it; a release build needs
  teksilo-render's `live-image-timings` feature.
- `TEKSILO_IDLE_TRACE=1` prints the window's wakes once a second while
  anything wakes it. It prints nothing while no producer commits.

## See also

- [Idle and animation](idle-and-animation.md): the zero-frame rule, and
  `RepaintTrigger` for off-thread content that is not pixels.
- [The widget's catalog page](widgets/live_image.md).
- [The design record](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/live-image.md):
  the protocol, the renderer's decisions and the measurements behind them.
