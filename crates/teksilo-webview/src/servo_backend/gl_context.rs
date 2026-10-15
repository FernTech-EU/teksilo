// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`HeadlessGlContext`]: the OpenGL context a Servo web view renders into,
//! offscreen, and the readback that hands its frames to a `LiveImage`.
//!
//! Servo ships three rendering contexts and none fits. `WindowRenderingContext`
//! and the `OffscreenRenderingContext` made from it put a GL context on the
//! window wgpu presents to, the conflict this backend exists to avoid.
//! `SoftwareRenderingContext` needs no window, but asks surfman for its
//! *software* adapter, which on Linux is llvmpipe: WebRender rasterised on the
//! CPU, on the one platform this backend targets. This context is that one
//! with the adapter made a choice: the hardware adapter first, the software one
//! when there is no hardware adapter (a CI runner, a machine without a GPU
//! driver).
//!
//! The readback does not wait for the GPU. [`HeadlessGlContext::begin_read`]
//! starts copying the frame just painted into a pixel-pack buffer and sets a
//! fence; [`HeadlessGlContext::finish_reads`] maps the newest copy whose fence
//! has signalled. The rows come out bottom-up, as OpenGL stores them, and are
//! handed on that way: the widget draws them turned upright
//! (`ImageOrientation::FlipVertical`) instead of this code copying every row
//! in reverse. Measured on a Radeon 890M, paint and readback of a full
//! 1920 x 1080 frame cost a median 1.9 ms of the UI thread this way, against
//! 3.5 ms for a plain `glReadPixels`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dpi::PhysicalSize;
use euclid::default::Size2D as UntypedSize2D;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RefreshDriver, RenderingContext, RgbaImage};
use surfman::chains::{PreserveBuffer, SwapChain};
use surfman::{
    Connection, Context, ContextAttributeFlags, ContextAttributes, Device, Error, GLApi, GLVersion,
    Surface, SurfaceAccess, SurfaceInfo, SurfaceTexture, SurfaceType,
};

/// Which kind of GPU adapter a context was made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdapterKind {
    Hardware,
    Software,
}

/// Readbacks in flight at most. Three keep the GPU busy on one while the UI
/// thread maps another and a third is free to start.
const READ_SLOTS: usize = 3;

/// One pixel-pack buffer and the read it holds.
struct ReadSlot {
    buffer: u32,
    /// The bytes the buffer was allocated for.
    capacity: usize,
    /// The size of the frame copied into it.
    size: (u32, u32),
    /// Signalled once the copy has finished on the GPU.
    fence: Option<gl::GLsync>,
}

/// The pixel-pack buffers and which of them hold a read, oldest first.
#[derive(Default)]
struct ReadRing {
    slots: Vec<ReadSlot>,
    pending: VecDeque<usize>,
}

/// A GL context with a surfman swap chain on a generic, windowless surface.
/// See the [module docs](self).
pub(super) struct HeadlessGlContext {
    size: Cell<PhysicalSize<u32>>,
    gleam: Rc<dyn Gl>,
    glow: Arc<glow::Context>,
    device: RefCell<Device>,
    context: RefCell<Context>,
    swap_chain: SwapChain<Device>,
    adapter: AdapterKind,
    driver: Rc<dyn RefreshDriver>,
    reads: RefCell<ReadRing>,
}

impl HeadlessGlContext {
    /// A context of `size` on the hardware adapter, or on the software one
    /// when that fails. `driver` paces Servo's animation frames.
    pub(super) fn new(
        size: PhysicalSize<u32>,
        driver: Rc<dyn RefreshDriver>,
    ) -> Result<Self, Error> {
        match Self::on_adapter(size, AdapterKind::Hardware, driver.clone()) {
            Ok(context) => Ok(context),
            Err(_) => Self::on_adapter(size, AdapterKind::Software, driver),
        }
    }

    fn on_adapter(
        size: PhysicalSize<u32>,
        kind: AdapterKind,
        driver: Rc<dyn RefreshDriver>,
    ) -> Result<Self, Error> {
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let connection = Connection::new()?;
        let adapter = match kind {
            AdapterKind::Hardware => connection.create_hardware_adapter()?,
            AdapterKind::Software => connection.create_software_adapter()?,
        };
        let device = connection.create_device(&adapter)?;
        let flags = ContextAttributeFlags::ALPHA
            | ContextAttributeFlags::DEPTH
            | ContextAttributeFlags::STENCIL;
        let gl_api = connection.gl_api();
        let version = match gl_api {
            GLApi::GLES => GLVersion { major: 3, minor: 0 },
            GLApi::GL => GLVersion { major: 3, minor: 2 },
        };
        let descriptor = device.create_context_descriptor(&ContextAttributes { flags, version })?;
        let mut context = device.create_context(&descriptor, None)?;
        // SAFETY: the loaders resolve symbols of the context just created,
        // which is the context these function tables are used with.
        let gleam: Rc<dyn Gl> = match gl_api {
            GLApi::GL => unsafe {
                gl::GlFns::load_with(|name| device.get_proc_address(&context, name))
            },
            GLApi::GLES => unsafe {
                gl::GlesFns::load_with(|name| device.get_proc_address(&context, name))
            },
        };
        // SAFETY: as above.
        let glow = unsafe {
            glow::Context::from_loader_function(|name| device.get_proc_address(&context, name))
        };
        let surface = device.create_surface(
            &context,
            SurfaceAccess::GPUOnly,
            SurfaceType::Generic {
                size: euclid::Size2D::new(size.width as i32, size.height as i32),
            },
        )?;
        device
            .bind_surface_to_context(&mut context, surface)
            .map_err(|(error, mut surface)| {
                let _ = device.destroy_surface(&mut context, &mut surface);
                error
            })?;
        device.make_context_current(&context)?;
        let swap_chain = SwapChain::create_attached(&device, &mut context, SurfaceAccess::GPUOnly)?;
        Ok(Self {
            size: Cell::new(size),
            gleam,
            glow: Arc::new(glow),
            device: RefCell::new(device),
            context: RefCell::new(context),
            swap_chain,
            adapter: kind,
            driver,
            reads: RefCell::new(ReadRing::default()),
        })
    }

    /// The adapter this context runs on.
    pub(super) fn adapter(&self) -> AdapterKind {
        self.adapter
    }

    /// The GL vendor and renderer strings, for diagnostics.
    pub(super) fn renderer(&self) -> String {
        let _ = self.make_current();
        format!(
            "{} / {}",
            self.gleam.get_string(gl::VENDOR),
            self.gleam.get_string(gl::RENDERER)
        )
    }

    fn framebuffer(&self) -> u32 {
        let device = self.device.borrow();
        let context = self.context.borrow();
        device
            .context_surface_info(&context)
            .unwrap_or(None)
            .and_then(|info| info.framebuffer_object)
            .map_or(0, |framebuffer| framebuffer.0.into())
    }

    /// Start copying the frame just painted out of the back buffer, without
    /// waiting for it. When every buffer already holds a read, the oldest is
    /// dropped: a frame painted since supersedes it.
    pub(super) fn begin_read(&self) {
        let gl = &self.gleam;
        let _ = self.make_current();
        let size = self.size.get();
        let bytes = size.width as usize * size.height as usize * 4;
        let mut ring = self.reads.borrow_mut();
        if ring.slots.is_empty() {
            ring.slots = gl
                .gen_buffers(READ_SLOTS as i32)
                .into_iter()
                .map(|buffer| ReadSlot {
                    buffer,
                    capacity: 0,
                    size: (0, 0),
                    fence: None,
                })
                .collect();
        }
        let index = match (0..ring.slots.len()).find(|i| !ring.pending.contains(i)) {
            Some(free) => free,
            None => {
                let oldest = ring.pending.pop_front().expect("every slot is pending");
                if let Some(fence) = ring.slots[oldest].fence.take() {
                    gl.delete_sync(fence);
                }
                oldest
            }
        };
        let framebuffer = self.framebuffer();
        let slot = &mut ring.slots[index];
        gl.bind_buffer(gl::PIXEL_PACK_BUFFER, slot.buffer);
        if slot.capacity != bytes {
            gl.buffer_data_untyped(
                gl::PIXEL_PACK_BUFFER,
                bytes as isize,
                std::ptr::null(),
                gl::STREAM_READ,
            );
            slot.capacity = bytes;
        }
        gl.bind_framebuffer(gl::FRAMEBUFFER, framebuffer);
        // OSMesa once failed to render after a readback with a vertex array
        // still bound (servo#18606); Servo's own readback unbinds it too.
        gl.bind_vertex_array(0);
        gl.pixel_store_i(gl::PACK_ALIGNMENT, 4);
        // SAFETY: a pixel-pack buffer of `bytes` is bound, so the read writes
        // into it at offset 0 and never through a client pointer.
        unsafe {
            gl.read_pixels_into_pbo(
                0,
                0,
                size.width as i32,
                size.height as i32,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
            );
        }
        slot.size = (size.width, size.height);
        slot.fence = Some(gl.fence_sync(gl::SYNC_GPU_COMMANDS_COMPLETE, 0));
        gl.bind_buffer(gl::PIXEL_PACK_BUFFER, 0);
        // Submit now, so the copy runs while the UI thread does other work.
        gl.flush();
        ring.pending.push_back(index);
    }

    /// Whether a read started with [`begin_read`](Self::begin_read) has not
    /// been handed on yet.
    pub(super) fn reads_pending(&self) -> bool {
        !self.reads.borrow().pending.is_empty()
    }

    /// Hand the newest finished read to `consume` as `(rows, width, height)`,
    /// rows bottom-up with no padding, and drop the reads older than it. When
    /// none has finished, wait up to `wait` for the newest.
    ///
    /// Returns whether a read was handed on.
    pub(super) fn finish_reads(
        &self,
        wait: Duration,
        consume: &mut dyn FnMut(&[u8], u32, u32),
    ) -> bool {
        let gl = &self.gleam;
        let mut ring = self.reads.borrow_mut();
        if ring.pending.is_empty() {
            return false;
        }
        let _ = self.make_current();
        // The GPU finishes copies in order, so the newest signalled fence
        // marks every older copy as done too.
        let signalled = |slot: &ReadSlot, timeout: u64| {
            slot.fence.is_some_and(|fence| {
                matches!(
                    gl.client_wait_sync(fence, gl::SYNC_FLUSH_COMMANDS_BIT, timeout),
                    gl::ALREADY_SIGNALED | gl::CONDITION_SATISFIED
                )
            })
        };
        let newest = ring
            .pending
            .iter()
            .rposition(|&i| signalled(&ring.slots[i], 0));
        let newest = match newest {
            Some(position) => position,
            None => {
                let last = ring.pending.len() - 1;
                let index = ring.pending[last];
                if !signalled(&ring.slots[index], wait.as_nanos() as u64) {
                    return false;
                }
                last
            }
        };
        let handed: Vec<usize> = ring.pending.drain(..=newest).collect();
        let index = *handed.last().expect("drained at least the newest");
        for &i in &handed {
            if let Some(fence) = ring.slots[i].fence.take() {
                gl.delete_sync(fence);
            }
        }
        let slot = &ring.slots[index];
        let (width, height) = slot.size;
        let len = width as usize * height as usize * 4;
        gl.bind_buffer(gl::PIXEL_PACK_BUFFER, slot.buffer);
        let mapped = gl.map_buffer_range(gl::PIXEL_PACK_BUFFER, 0, len as isize, gl::MAP_READ_BIT);
        let handed_on = !mapped.is_null();
        if handed_on {
            // SAFETY: the buffer holds `len` bytes, mapped for reading until
            // the unmap below, and nothing writes it meanwhile: its fence has
            // signalled and no new read is started while it is mapped.
            let rows = unsafe { std::slice::from_raw_parts(mapped as *const u8, len) };
            consume(rows, width, height);
            gl.unmap_buffer(gl::PIXEL_PACK_BUFFER);
        }
        gl.bind_buffer(gl::PIXEL_PACK_BUFFER, 0);
        handed_on
    }
}

impl Drop for HeadlessGlContext {
    fn drop(&mut self) {
        let _ = self.make_current();
        let ring = self.reads.get_mut();
        for slot in &mut ring.slots {
            if let Some(fence) = slot.fence.take() {
                self.gleam.delete_sync(fence);
            }
        }
        let buffers: Vec<u32> = ring.slots.iter().map(|slot| slot.buffer).collect();
        if !buffers.is_empty() {
            self.gleam.delete_buffers(&buffers);
        }
        let device = &mut self.device.borrow_mut();
        let context = &mut self.context.borrow_mut();
        let _ = self.swap_chain.destroy(device, context);
        let _ = device.destroy_context(context);
    }
}

impl RenderingContext for HeadlessGlContext {
    fn prepare_for_rendering(&self) {
        let framebuffer = self.framebuffer();
        self.gleam.bind_framebuffer(gl::FRAMEBUFFER, framebuffer);
    }

    /// A synchronous read of `rect`, top row first: Servo's own path for a
    /// screenshot, not the one frames take to the widget.
    fn read_to_image(&self, rect: DeviceIntRect) -> Option<RgbaImage> {
        let _ = self.make_current();
        let size = self.size.get();
        let (x, width, height) = (rect.min.x, rect.width(), rect.height());
        if width <= 0 || height <= 0 {
            return None;
        }
        // GL rows count from the bottom.
        let y = size.height as i32 - rect.max.y;
        let framebuffer = self.framebuffer();
        self.gleam.bind_framebuffer(gl::FRAMEBUFFER, framebuffer);
        self.gleam.bind_vertex_array(0);
        let pixels = self
            .gleam
            .read_pixels(x, y, width, height, gl::RGBA, gl::UNSIGNED_BYTE);
        let stride = width as usize * 4;
        let upright: Vec<u8> = pixels
            .chunks_exact(stride)
            .rev()
            .flatten()
            .copied()
            .collect();
        RgbaImage::from_raw(width as u32, height as u32, upright)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        if self.size.get() == size {
            return;
        }
        self.size.set(size);
        let device = &mut self.device.borrow_mut();
        let context = &mut self.context.borrow_mut();
        let _ = self.swap_chain.resize(
            device,
            context,
            euclid::Size2D::new(size.width as i32, size.height as i32),
        );
    }

    fn present(&self) {
        let device = &mut self.device.borrow_mut();
        let context = &mut self.context.borrow_mut();
        let _ = self
            .swap_chain
            .swap_buffers(device, context, PreserveBuffer::No);
    }

    fn make_current(&self) -> Result<(), Error> {
        let device = self.device.borrow();
        let context = self.context.borrow();
        device.make_context_current(&context)
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.gleam.clone()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.glow.clone()
    }

    fn create_texture(
        &self,
        surface: Surface,
    ) -> Option<(SurfaceTexture, u32, UntypedSize2D<i32>)> {
        let device = &self.device.borrow();
        let context = &mut self.context.borrow_mut();
        let SurfaceInfo { size, .. } = device.surface_info(&surface);
        let texture = device.create_surface_texture(context, surface).ok()?;
        let id = device
            .surface_texture_object(&texture)
            .map(|object| object.0.get())
            .unwrap_or(0);
        Some((texture, id, size))
    }

    fn destroy_texture(&self, texture: SurfaceTexture) -> Option<Surface> {
        let device = &self.device.borrow();
        let context = &mut self.context.borrow_mut();
        device
            .destroy_surface_texture(context, texture)
            .map_err(|(error, _)| error)
            .ok()
    }

    fn connection(&self) -> Option<Connection> {
        Some(self.device.borrow().connection())
    }

    fn refresh_driver(&self) -> Option<Rc<dyn RefreshDriver>> {
        Some(self.driver.clone())
    }
}
