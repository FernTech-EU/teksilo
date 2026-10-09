// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the live passes of one device hold: each window's current texture
//! of each source, with the generation it holds. A window whose texture
//! lacks a generation another window on the device already holds copies
//! that window's texture on the device, instead of copying the frame again
//! out of the source's buffer: no lock, and no copy on the UI thread.
//!
//! A pass copies only from passes that published from its own thread. A
//! window renders whole on that thread, from its live pass to its
//! submission, so no other window there writes the texture between the
//! copy's recording and its submission, and the queue runs the writes that
//! filled the texture before the copy.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::ThreadId;

use super::LiveImageId;

/// The live textures of one device, shared by every pass on it. `Clone`
/// shares it. `H` is what a pass keeps of another pass's texture to copy
/// it: a handle for the GPU, the bytes for the mirror.
pub struct DeviceTextures<H> {
    passes: Arc<Mutex<Vec<Published<H>>>>,
}

impl<H> DeviceTextures<H> {
    /// The table of a device no pass has joined yet.
    pub fn new() -> Self {
        Self {
            passes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// How many passes hold a texture in it.
    #[doc(hidden)]
    pub fn passes(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Published<H>>> {
        self.passes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<H> Default for DeviceTextures<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> Clone for DeviceTextures<H> {
    fn clone(&self) -> Self {
        Self {
            passes: Arc::clone(&self.passes),
        }
    }
}

impl<H> std::fmt::Debug for DeviceTextures<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceTextures")
            .field("passes", &self.passes())
            .finish()
    }
}

/// What one pass holds, from the thread it rendered on.
struct Published<H> {
    pass: u64,
    thread: ThreadId,
    held: Vec<Held<H>>,
}

/// One current texture of a pass.
pub(crate) struct Held<H> {
    pub(crate) id: LiveImageId,
    pub(crate) size: (u32, u32),
    pub(crate) generation: u64,
    pub(crate) texture: H,
}

/// One pass's place in its device's table, left when it drops.
pub(crate) struct Siblings<H> {
    table: DeviceTextures<H>,
    pass: u64,
    /// The table holds textures of this pass.
    listed: bool,
}

impl<H: Clone> Siblings<H> {
    pub(crate) fn join(table: &DeviceTextures<H>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            table: table.clone(),
            pass: NEXT.fetch_add(1, Ordering::Relaxed),
            listed: false,
        }
    }

    pub(crate) fn table(&self) -> &DeviceTextures<H> {
        &self.table
    }

    /// Replace what this pass holds with `held`, published from this thread.
    /// A pass that holds nothing and held nothing takes no lock.
    pub(crate) fn publish(&mut self, held: Vec<Held<H>>) {
        if held.is_empty() && !self.listed {
            return;
        }
        self.listed = !held.is_empty();
        let mut passes = self.table.lock();
        let at = passes.iter().position(|p| p.pass == self.pass);
        match (at, held.is_empty()) {
            (Some(at), true) => {
                passes.swap_remove(at);
            }
            (Some(at), false) => {
                passes[at].thread = std::thread::current().id();
                passes[at].held = held;
            }
            (None, true) => {}
            (None, false) => passes.push(Published {
                pass: self.pass,
                thread: std::thread::current().id(),
                held,
            }),
        }
    }

    /// A texture another pass of this thread holds of source `id`, at `size`
    /// and generation `generation`.
    pub(crate) fn find(&self, id: LiveImageId, size: (u32, u32), generation: u64) -> Option<H> {
        let thread = std::thread::current().id();
        let passes = self.table.lock();
        passes
            .iter()
            .filter(|p| p.pass != self.pass && p.thread == thread)
            .flat_map(|p| &p.held)
            .find(|h| h.id == id && h.size == size && h.generation == generation)
            .map(|h| h.texture.clone())
    }
}

impl<H> Drop for Siblings<H> {
    /// The pass goes, and its textures with it: none stays alive for a copy.
    fn drop(&mut self) {
        self.table.lock().retain(|p| p.pass != self.pass);
    }
}
