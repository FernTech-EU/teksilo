// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The flags every wgpu instance in this workspace is built with.
//!
//! Two instances exist: the one [`crate::test_support`] opens for offscreen
//! work, and the one `teksilo-platform` opens for windows. They have to agree
//! about this, so the rule lives here, in the crate both can reach, rather
//! than at either call site.

/// The `InstanceFlags` to build a wgpu instance with: wgpu's own defaults,
/// read from the environment, minus `VALIDATION_INDIRECT_CALL`.
///
/// That flag is set in **release** builds too — it comes from
/// `InstanceFlags::from_build_config`'s non-debug branch, so it is not a
/// debug-only cost. It makes `Device::new` build a set of compute and render
/// pipelines that validate the arguments of indirect draws. This renderer
/// issues no indirect draws at all — not one `draw_indirect`,
/// `dispatch_indirect` or `multi_draw_*` anywhere in the workspace — so those
/// pipelines check nothing we will ever submit.
///
/// That alone would only be wasted startup work. The reason the flag is
/// cleared is that building those pipelines is also a way for device creation
/// to *fail*, and failing there is not survivable.
/// `wgpu_core::device::resource::Device::new` creates the hal device, then its
/// `empty_bgl` — which registers a bind-group layout with the Vulkan backend's
/// `DescriptorAllocator` — and only then calls `IndirectValidation::new(..)?`.
/// A driver that cannot build them takes that `?`, and the early return drops
/// the hal device *without* unregistering `empty_bgl`, because hal objects are
/// not RAII and need an explicit destroy. `Drop for DescriptorAllocator` then
/// finds a non-empty bucket and panics — "buckets are not empty, at least one
/// BGL has not been unregistered" — from an ordinary, non-unwinding drop, so
/// its own `thread::panicking()` guard does not suppress it.
///
/// The process therefore dies *inside* `request_device`, and neither caller
/// can do anything about it there: a backend search cannot search past a
/// panic, and an offscreen renderer cannot fall back to a software adapter.
/// Reported from the field on an older Windows 10 machine where an app never
/// opened a window, and confirmed there by setting
/// `WGPU_VALIDATION_INDIRECT_CALL=0`, which let the window open. D3D12 has no
/// `DescriptorAllocator` and never runs the assertion, which is why forcing
/// `WGPU_BACKEND=dx12` looked like a graphics fix when it was really a way of
/// not reaching this code.
///
/// `WGPU_VALIDATION_INDIRECT_CALL` is honoured in both directions, so the flag
/// stays reachable for anyone debugging wgpu itself.
#[must_use]
pub fn instance_flags() -> wgpu::InstanceFlags {
    without_unused_indirect_validation(
        wgpu::InstanceFlags::from_env_or_default(),
        std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_some(),
    )
}

/// [`instance_flags`]'s decision, as a pure function of its two inputs.
///
/// Split out because the alternative is a test that writes a process-wide
/// environment variable: unsafe since the 2024 edition, and racy against every
/// other test in the binary regardless.
///
/// `asked_for` is whether the variable is *present*, not whether it is true.
/// wgpu's `with_env` has already read its value into `base` by this point — it
/// sets the flag for any value but `0`, and clears it for `0` — so all this
/// has to decide is whether the user expressed an opinion at all. Testing the
/// resulting bit instead cannot tell "wgpu's default" from "a developer asked
/// for it", and testing the value again would re-implement `with_env`.
fn without_unused_indirect_validation(
    base: wgpu::InstanceFlags,
    asked_for: bool,
) -> wgpu::InstanceFlags {
    if asked_for {
        return base;
    }
    let mut flags = base;
    flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDIRECT: wgpu::InstanceFlags = wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL;

    #[test]
    fn an_unset_variable_clears_the_flag_wgpu_defaults_to() {
        // What a release build gets: `from_build_config` sets the flag and
        // nobody asked for it.
        assert!(!without_unused_indirect_validation(INDIRECT, false).contains(INDIRECT));
    }

    #[test]
    fn an_explicit_request_is_honoured_in_both_directions() {
        // `with_env` has already applied the value, so the flag as it arrives
        // is the answer — set for `WGPU_VALIDATION_INDIRECT_CALL=1`, cleared
        // for `=0`. Clearing a `=0` again would be right by luck; re-setting a
        // `=1` would silently ignore the developer.
        assert!(without_unused_indirect_validation(INDIRECT, true).contains(INDIRECT));
        assert!(
            !without_unused_indirect_validation(wgpu::InstanceFlags::empty(), true)
                .contains(INDIRECT)
        );
    }

    #[test]
    fn no_other_flag_is_touched() {
        let base = wgpu::InstanceFlags::DEBUG | wgpu::InstanceFlags::VALIDATION | INDIRECT;
        let got = without_unused_indirect_validation(base, false);
        assert_eq!(got, base.difference(INDIRECT));
    }
}
