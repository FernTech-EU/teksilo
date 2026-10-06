// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether a GPU device is still alive.
//!
//! wgpu reports a lost device through the one callback a device holds, and
//! drops the error otherwise. Teksilo shares one device between every window
//! (and every offscreen renderer) of a process, so the latch belongs to the
//! device: [`DeviceHealth::install`] sets the callback once, where the device
//! is opened, and every renderer built on that device reads the same flag
//! (`Renderer::with_device_health`). A device the process destroys on purpose
//! is not lost.
//!
//! The latch is carried with the device rather than looked up by it: wgpu
//! compares devices by an id that two instances in one process can share, so
//! a registry keyed by `wgpu::Device` would let one device's loss mark
//! another.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The lost-device latch of one device. `Clone` shares it.
#[derive(Clone, Debug)]
pub struct DeviceHealth {
    lost: Arc<AtomicBool>,
}

impl DeviceHealth {
    /// Install a lost-device latch on `device`. It replaces any lost-device
    /// callback the device had: install it once per device, where it is
    /// opened, and hand the latch to every renderer on that device.
    pub fn install(device: &wgpu::Device) -> Self {
        let health = Self {
            lost: Arc::new(AtomicBool::new(false)),
        };
        let lost = health.lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            if reason == wgpu::DeviceLostReason::Destroyed {
                return;
            }
            if !lost.swap(true, Ordering::AcqRel) {
                eprintln!(
                    "teksilo-render: the GPU device was lost ({message}): nothing more reaches \
                     the screen, and live images stop counting frames"
                );
            }
        });
        health
    }

    /// Whether the device was lost. Once it is, it stays lost.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Whether `self` and `other` are the latch of one device.
    pub fn same_device(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.lost, &other.lost)
    }

    /// Test hook: behave as if the device were lost.
    #[doc(hidden)]
    pub fn mark_lost_for_testing(&self) {
        self.lost.store(true, Ordering::Release);
    }
}
