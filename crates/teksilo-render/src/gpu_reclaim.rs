// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Getting dropped textures freed while nothing is drawn.
//!
//! Dropping a `wgpu::Texture` only schedules its destruction: wgpu frees it
//! at the first device maintenance after the last submission that used it
//! completes, and a submission runs one. A window that stops drawing (the
//! live image it showed went away, the window closed) submits nothing more,
//! so its freed textures would wait for some other window's frame. A
//! renderer that drops textures records here its device and its last
//! submission; [`poll_gpu_reclaim`], which the event loop calls when it is
//! about to sleep, polls each recorded device without blocking until that
//! submission is complete.

use std::sync::Mutex;

use crate::device_health::DeviceHealth;

/// A device with textures waiting to be freed, and the submission after
/// which they can be.
struct Pending {
    /// Identifies the device: wgpu's own device comparison can mistake two
    /// instances' devices for one (see [`DeviceHealth`]).
    health: DeviceHealth,
    device: wgpu::Device,
    submission: wgpu::SubmissionIndex,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

/// The device `health` latches dropped textures that `submission`, its
/// renderer's last, may still use. Merges with a pending record for the same
/// device: submissions complete in order, so the later one covers both.
pub fn flag_device(
    health: &DeviceHealth,
    device: &wgpu::Device,
    submission: wgpu::SubmissionIndex,
) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    match pending.iter_mut().find(|p| p.health.same_device(health)) {
        Some(p) => p.submission = submission,
        None => pending.push(Pending {
            health: health.clone(),
            device: device.clone(),
            submission,
        }),
    }
}

/// Poll every device with textures waiting to be freed, without blocking.
/// Returns `true` while another poll is needed: some submission is still on
/// the GPU. The event loop then wakes again shortly to poll once more; no
/// frame is drawn for it.
pub fn poll_gpu_reclaim() -> bool {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    pending.retain(|p| {
        if p.health.is_lost() {
            return false;
        }
        let done = p.device.poll(wgpu::PollType::Wait {
            submission_index: Some(p.submission.clone()),
            timeout: Some(std::time::Duration::ZERO),
        });
        // Done: nothing more to wait for. Only a timeout keeps the record.
        matches!(done, Err(wgpu::PollError::Timeout))
    });
    !pending.is_empty()
}

/// How many devices wait for a reclaim poll.
#[doc(hidden)]
pub fn pending_reclaims() -> usize {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).len()
}
