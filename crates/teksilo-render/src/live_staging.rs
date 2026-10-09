// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The staging a live picture's uploads are copied into: buffers kept mapped
//! and reused from one frame to the next.
//!
//! `Queue::write_texture` stages each call in a buffer it creates for that
//! call. On Metal that is a new `MTLBuffer`, allocated and first touched
//! while the live pass holds the source's lock: on an Apple M4, a 1080p
//! write took 2.7 ms at p50 and 3.7 ms at p99 at 60 Hz, against 1.0 and
//! 1.55 ms for the same copy into a buffer already mapped. Vulkan pools that
//! memory, which hides the cost there.
//!
//! So the backend copies each band into a [chunk](Chunk) of a
//! `MAP_WRITE | COPY_SRC` buffer that is already mapped, under the lock, and
//! records a `copy_buffer_to_texture` for the frame's own encoder. Before the
//! submission the chunks written are unmapped ([`StagingPool::finish_frame`]);
//! after it they are mapped again ([`StagingPool::after_submit`]), which wgpu
//! completes once the GPU is done with them, and the pool takes them back at
//! its next use. A chunk unused for [`IDLE_LIFETIME`] is freed, so a stream
//! that stops gives its staging back.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// The size of a chunk: the largest band a write may take
/// ([`BAND_BYTES`](teksilo_canvas::live_image::internal::BAND_BYTES)), so a
/// 1080p frame fits one. A larger band gets a chunk of its own size.
pub(crate) const CHUNK_SIZE: u64 = teksilo_canvas::live_image::internal::BAND_BYTES;

/// How long a chunk may go unused before it is freed.
pub(crate) const IDLE_LIFETIME: Duration = Duration::from_secs(1);

/// Where a band starts in a chunk: what `copy_buffer_to_texture` accepts for
/// any texel size, and `get_mapped_range_mut`'s own alignment.
const OFFSET_ALIGNMENT: u64 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;

/// Where a chunk is in its cycle.
enum ChunkState {
    /// Mapped, nothing written since it came back.
    Free,
    /// Mapped, written this frame up to `used` bytes.
    Writing { used: u64 },
    /// Unmapped for this frame's submission, to be mapped again after it.
    Submitted,
    /// Mapping again; `mapped` turns [`MAPPED`] once the GPU is done with
    /// it, or [`MAP_FAILED`] (a lost device), and the chunk is dropped.
    Mapping { mapped: Arc<AtomicU8> },
}

const MAPPING: u8 = 0;
const MAPPED: u8 = 1;
const MAP_FAILED: u8 = 2;

/// One staging buffer.
struct Chunk {
    buffer: wgpu::Buffer,
    size: u64,
    state: ChunkState,
    /// When it last took a band.
    last_used: Instant,
}

/// What a band was copied into: the chunk's buffer and the offset there.
pub(crate) struct Staged {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) offset: u64,
}

/// What the pool holds and did, for tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StagingStats {
    /// Chunks held now, in any state.
    pub chunks: usize,
    /// Their bytes.
    pub bytes: u64,
    /// Chunks created since the pool was.
    pub created: u64,
    /// Chunks freed for going unused.
    pub freed: u64,
}

/// The per-renderer pool of mapped staging buffers.
#[derive(Default)]
pub(crate) struct StagingPool {
    chunks: Vec<Chunk>,
    created: u64,
    freed: u64,
    /// Test hook: chunks go as soon as they are unused.
    idle_lifetime: Option<Duration>,
    /// Test hook: the next chunk creation reports no memory.
    fail_next: bool,
}

impl StagingPool {
    /// Stage `bytes` bytes in a mapped chunk, written by `fill` into the
    /// write-only view of them it is handed; `None` when no chunk can be made
    /// (the device has no memory for one) or mapped, and the caller writes
    /// some other way.
    pub(crate) fn stage(
        &mut self,
        device: &wgpu::Device,
        bytes: u64,
        fill: impl FnOnce(&mut wgpu::BufferViewMut),
    ) -> Option<Staged> {
        let now = Instant::now();
        let at = match self.room_for(bytes) {
            Some(at) => at,
            None => {
                // Chunks the GPU is done with come back on a maintenance;
                // look once before making another.
                let _ = device.poll(wgpu::PollType::Poll);
                self.take_back();
                match self.room_for(bytes) {
                    Some(at) => at,
                    None => self.create(device, bytes.max(CHUNK_SIZE), now)?,
                }
            }
        };
        let chunk = &mut self.chunks[at];
        let offset = match chunk.state {
            ChunkState::Writing { used } => used.next_multiple_of(OFFSET_ALIGNMENT),
            _ => 0,
        };
        {
            let mut view = chunk
                .buffer
                .slice(offset..offset + bytes)
                .get_mapped_range_mut()
                .ok()?;
            fill(&mut view);
        }
        chunk.state = ChunkState::Writing {
            used: offset + bytes,
        };
        chunk.last_used = now;
        Some(Staged {
            buffer: chunk.buffer.clone(),
            offset,
        })
    }

    /// Unmap every chunk written this frame: a buffer must not be mapped
    /// when a submission uses it. Before the frame's copies are recorded.
    pub(crate) fn finish_frame(&mut self) {
        for chunk in &mut self.chunks {
            if matches!(chunk.state, ChunkState::Writing { .. }) {
                chunk.buffer.unmap();
                chunk.state = ChunkState::Submitted;
            }
        }
    }

    /// After the frame's submission: map the chunks it used again (wgpu
    /// completes it once the GPU is done with them), and free the chunks
    /// unused for [`IDLE_LIFETIME`]. Whether any was freed.
    pub(crate) fn after_submit(&mut self) -> bool {
        for chunk in &mut self.chunks {
            if matches!(chunk.state, ChunkState::Submitted) {
                let mapped = Arc::new(AtomicU8::new(MAPPING));
                let done = Arc::clone(&mapped);
                chunk
                    .buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Write, move |result| {
                        let state = if result.is_ok() { MAPPED } else { MAP_FAILED };
                        done.store(state, Ordering::Release);
                    });
                chunk.state = ChunkState::Mapping { mapped };
            }
        }
        self.take_back();
        let now = Instant::now();
        let lifetime = self.idle_lifetime.unwrap_or(IDLE_LIFETIME);
        let before = self.chunks.len();
        self.chunks.retain(|chunk| {
            !(matches!(chunk.state, ChunkState::Free)
                && now.saturating_duration_since(chunk.last_used) >= lifetime)
        });
        let freed = before - self.chunks.len();
        self.freed += freed as u64;
        freed > 0
    }

    pub(crate) fn stats(&self) -> StagingStats {
        StagingStats {
            chunks: self.chunks.len(),
            bytes: self.chunks.iter().map(|c| c.size).sum(),
            created: self.created,
            freed: self.freed,
        }
    }

    pub(crate) fn set_idle_lifetime(&mut self, lifetime: Duration) {
        self.idle_lifetime = Some(lifetime);
    }

    pub(crate) fn fail_next(&mut self) {
        self.fail_next = true;
    }

    /// A chunk with `bytes` free: one already written this frame with room
    /// after it, else a free one large enough.
    fn room_for(&self, bytes: u64) -> Option<usize> {
        let writing = self.chunks.iter().position(|c| match c.state {
            ChunkState::Writing { used } => {
                used.next_multiple_of(OFFSET_ALIGNMENT) + bytes <= c.size
            }
            _ => false,
        });
        writing.or_else(|| {
            self.chunks
                .iter()
                .position(|c| matches!(c.state, ChunkState::Free) && bytes <= c.size)
        })
    }

    /// Chunks whose mapping completed are free again; one whose mapping
    /// failed goes.
    fn take_back(&mut self) {
        self.chunks.retain(|chunk| match &chunk.state {
            ChunkState::Mapping { mapped } => mapped.load(Ordering::Acquire) != MAP_FAILED,
            _ => true,
        });
        for chunk in &mut self.chunks {
            if let ChunkState::Mapping { mapped } = &chunk.state
                && mapped.load(Ordering::Acquire) == MAPPED
            {
                chunk.state = ChunkState::Free;
            }
        }
    }

    /// Drop every chunk: the renderer holds no live texture, so nothing will
    /// be uploaded soon. A chunk a submission still uses lives until it ends.
    /// Whether any was held.
    pub(crate) fn release_all(&mut self) -> bool {
        let held = !self.chunks.is_empty();
        self.freed += self.chunks.len() as u64;
        self.chunks.clear();
        held
    }

    /// A new chunk of `size` bytes, mapped, inside an out-of-memory scope: a
    /// device that cannot hold it reports it as a value, not to wgpu's
    /// default handler, which panics.
    fn create(&mut self, device: &wgpu::Device, size: u64, now: Instant) -> Option<usize> {
        if std::mem::take(&mut self.fail_next) {
            return None;
        }
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live_image_staging"),
            size,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        // A native backend's pop returns a ready future.
        if pollster::block_on(memory.pop()).is_some() {
            return None;
        }
        self.created += 1;
        self.chunks.push(Chunk {
            buffer,
            size,
            state: ChunkState::Free,
            last_used: now,
        });
        Some(self.chunks.len() - 1)
    }
}
