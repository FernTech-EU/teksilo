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
//! about to sleep, polls each recorded device without blocking until every
//! submission recorded for it is complete.

use std::sync::Mutex;

use crate::device_health::DeviceHealth;

/// A device with textures waiting to be freed, and the submissions after
/// which they can be.
struct Pending {
    /// Identifies the device: wgpu's own device comparison can mistake two
    /// instances' devices for one (see [`DeviceHealth`]).
    health: DeviceHealth,
    device: wgpu::Device,
    /// Every flagged submission not yet seen complete. A later flag does not
    /// replace an earlier one: renderers sharing the device flag in the
    /// order they drop textures, not in the order they submitted (a window
    /// that closes flags its last frame, which may be older than another
    /// window's), and wgpu does not let two submissions be compared.
    submissions: Vec<wgpu::SubmissionIndex>,
}

/// Whether `submission` is no longer on the GPU, without waiting: complete,
/// or on a device that can no longer run it.
fn finished(device: &wgpu::Device, submission: &wgpu::SubmissionIndex) -> bool {
    let polled = device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission.clone()),
        timeout: Some(std::time::Duration::ZERO),
    });
    // Only a timeout says it still runs.
    !matches!(polled, Err(wgpu::PollError::Timeout))
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

/// The device `health` latches dropped textures that `submission`, its
/// renderer's last, may still use. Joins a pending record for the same
/// device, first forgetting the submissions of it already complete, so a
/// record holds no more than the flagged work still on the GPU, even where
/// nothing calls [`poll_gpu_reclaim`].
pub fn flag_device(
    health: &DeviceHealth,
    device: &wgpu::Device,
    submission: wgpu::SubmissionIndex,
) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    match pending.iter_mut().find(|p| p.health.same_device(health)) {
        Some(p) => {
            let device = &p.device;
            p.submissions.retain(|s| !finished(device, s));
            p.submissions.push(submission);
        }
        None => pending.push(Pending {
            health: health.clone(),
            device: device.clone(),
            submissions: vec![submission],
        }),
    }
}

/// Poll every device with textures waiting to be freed, without blocking.
/// Returns `true` while another poll is needed: some submission is still on
/// the GPU. The event loop then wakes again shortly to poll once more; no
/// frame is drawn for it.
pub fn poll_gpu_reclaim() -> bool {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    pending.retain_mut(|p| {
        if p.health.is_lost() {
            return false;
        }
        let device = &p.device;
        p.submissions.retain(|s| !finished(device, s));
        // Done: nothing more to wait for.
        !p.submissions.is_empty()
    });
    !pending.is_empty()
}

/// How many devices wait for a reclaim poll.
#[doc(hidden)]
pub fn pending_reclaims() -> usize {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).len()
}
