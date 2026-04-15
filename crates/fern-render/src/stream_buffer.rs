//! Persistent per-pipeline streaming buffers.
//!
//! Each frame, the renderer accumulates vertex data into per-pipeline
//! batches and flushes them to the GPU. The historical code path called
//! `device.create_buffer_init` on every flush, allocating and dropping
//! a fresh `wgpu::Buffer` pair (vertex + index) per batch — a known
//! antipattern that causes driver allocation pressure at high glyph
//! counts.
//!
//! [`StreamBuffer`] owns a single growable GPU buffer that persists
//! across frames. [`StreamBuffer::ensure_capacity`] is called once per
//! frame with the worst-case byte count (requires `&mut self`), growing
//! the underlying buffer if needed. After that, [`StreamBuffer::write`]
//! copies a batch to the GPU at the current write offset and advances
//! the cursor — all via interior mutability, so calls can interleave
//! freely with other `&self` methods on the renderer.

use std::cell::Cell;

/// A growable, ring-reset GPU buffer for streaming vertex or index data
/// across contiguous batches within a single frame.
///
/// Capacity growth requires `&mut self`; the per-batch `write` path only
/// needs `&self` thanks to a `Cell<u64>` cursor, so it composes cleanly
/// with methods that borrow other parts of the renderer immutably.
pub struct StreamBuffer {
    buffer: Option<wgpu::Buffer>,
    capacity_bytes: u64,
    write_offset: Cell<u64>,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl StreamBuffer {
    pub fn new(usage: wgpu::BufferUsages, label: &'static str) -> Self {
        Self {
            buffer: None,
            capacity_bytes: 0,
            write_offset: Cell::new(0),
            usage,
            label,
        }
    }

    /// Reset the write cursor to the start of the buffer.
    ///
    /// Call once at the top of `render()` — previous frame's contents
    /// are orphaned and overwritten in place.
    pub fn reset(&self) {
        self.write_offset.set(0);
    }

    /// Grow the underlying buffer if `required_bytes` exceeds the current
    /// capacity. Growth rounds up to the next power of two (minimum 1 KiB)
    /// to amortize reallocation. If the existing buffer already fits, this
    /// is a no-op.
    pub fn ensure_capacity(&mut self, device: &wgpu::Device, required_bytes: u64) {
        if required_bytes <= self.capacity_bytes && self.buffer.is_some() {
            return;
        }
        let new_cap = required_bytes.max(1024).next_power_of_two();
        self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: new_cap,
            usage: self.usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.capacity_bytes = new_cap;
    }

    /// Upload `data` at the current write cursor and advance the cursor.
    ///
    /// Returns `Some((buffer, offset, len_bytes))` on success, or `None`
    /// if [`ensure_capacity`](Self::ensure_capacity) was never called. The
    /// caller should slice the returned buffer at `offset..offset + len`
    /// when binding.
    ///
    /// Panics if `data.len() + write_offset > capacity_bytes`; the caller
    /// must size the buffer correctly up-front via `ensure_capacity`.
    pub fn write(
        &self,
        queue: &wgpu::Queue,
        data: &[u8],
    ) -> Option<(&wgpu::Buffer, u64, u64)> {
        let buf = self.buffer.as_ref()?;
        let offset = self.write_offset.get();
        let len = data.len() as u64;
        debug_assert!(
            offset + len <= self.capacity_bytes,
            "StreamBuffer overflow: {} + {} > {} ({})",
            offset,
            len,
            self.capacity_bytes,
            self.label
        );
        queue.write_buffer(buf, offset, data);
        self.write_offset.set(offset + len);
        Some((buf, offset, len))
    }
}

/// All per-pipeline streaming buffers owned by a [`Renderer`].
///
/// Grouped so the renderer can `reset()` them all at once and so macros
/// can pass a single handle around the render loop.
pub struct StreamBuffers {
    pub rect: StreamBuffer,
    pub sdf: StreamBuffer,
    pub quad: StreamBuffer,
    pub shadow: StreamBuffer,
    /// Shared index buffer — quad indices are deterministic so one buffer
    /// serves every pipeline that renders quads.
    pub index: StreamBuffer,
}

impl StreamBuffers {
    pub fn new() -> Self {
        Self {
            rect: StreamBuffer::new(wgpu::BufferUsages::VERTEX, "rect_stream"),
            sdf: StreamBuffer::new(wgpu::BufferUsages::VERTEX, "sdf_stream"),
            quad: StreamBuffer::new(wgpu::BufferUsages::VERTEX, "quad_stream"),
            shadow: StreamBuffer::new(wgpu::BufferUsages::VERTEX, "shadow_stream"),
            index: StreamBuffer::new(wgpu::BufferUsages::INDEX, "index_stream"),
        }
    }

    pub fn reset(&self) {
        self.rect.reset();
        self.sdf.reset();
        self.quad.reset();
        self.shadow.reset();
        self.index.reset();
    }
}

impl Default for StreamBuffers {
    fn default() -> Self {
        Self::new()
    }
}
