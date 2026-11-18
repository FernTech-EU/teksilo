// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Who a Windows named pipe is for: an access list granting the current user
//! alone, and the user a process runs as.
//!
//! Shared by the automation bridge's transport and the single-instance
//! election, the two places this crate makes a pipe. `CreateNamedPipeW` with
//! no security attributes grants read access to Everyone and to the
//! anonymous account, so both build their descriptor here.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
#[cfg(feature = "single-instance")]
use windows::Win32::Security::EqualSid;
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
#[cfg(feature = "single-instance")]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::core::{PCWSTR, PWSTR};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An owner-only security descriptor, freed on drop.
pub(crate) struct OwnerOnlySecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl OwnerOnlySecurity {
    /// Build `D:P(A;;GA;;;<current user SID>)`.
    ///
    /// `GA` (generic all) rather than a narrower mask because the only
    /// principal on the ACL is the creating user, and creating a *second*
    /// instance of the pipe needs `FILE_CREATE_PIPE_INSTANCE`: a narrower
    /// grant would lock the server out of its own next `accept`.
    pub(crate) fn current_user() -> io::Result<Self> {
        let sid = current_user_sid_string()?;
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a NUL-terminated UTF-16 buffer that outlives the
        // call; the descriptor is an out-parameter we own and free on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|e| io::Error::other(format!("building the pipe security descriptor: {e}")))?;
        Ok(Self { descriptor })
    }

    /// Attributes pointing at this descriptor: valid while `self` lives.
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for OwnerOnlySecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_invalid() {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

/// The user a process token runs as: the `TOKEN_USER` the system wrote, with
/// the SID it points at inside the same buffer.
pub(crate) struct ProcessUser {
    /// `u64`s rather than bytes, so the `TOKEN_USER` at its start (which
    /// holds a pointer) is aligned.
    buf: Vec<u64>,
}

impl ProcessUser {
    /// The user of the current process.
    pub(crate) fn current() -> io::Result<Self> {
        let mut token = HANDLE::default();
        // SAFETY: the pseudo handle of the current process needs no closing;
        // `token` is an out-parameter, owned below as soon as it is set.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|e| io::Error::other(format!("opening the process token: {e}")))?;
        // SAFETY: a token handle just opened, owned by nothing else.
        let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
        Self::of_token(&token)
    }

    /// The user process `pid` runs as. Fails when the process cannot be
    /// opened for that (it ended, or it runs elevated and this one does not).
    #[cfg(feature = "single-instance")]
    pub(crate) fn of_process(pid: u32) -> io::Result<Self> {
        // SAFETY: plain values in, a new handle out, owned below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| io::Error::other(format!("opening process {pid}: {e}")))?;
        // SAFETY: a process handle just opened, owned by nothing else.
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
        let mut token = HANDLE::default();
        // SAFETY: `process` is open for the whole call; `token` is an
        // out-parameter, owned below as soon as it is set.
        unsafe { OpenProcessToken(HANDLE(process.as_raw_handle()), TOKEN_QUERY, &mut token) }
            .map_err(|e| io::Error::other(format!("opening the token of process {pid}: {e}")))?;
        // SAFETY: a token handle just opened, owned by nothing else.
        let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
        Self::of_token(&token)
    }

    fn of_token(token: &OwnedHandle) -> io::Result<Self> {
        let token = HANDLE(token.as_raw_handle());
        // Size probe: expected to fail with ERROR_INSUFFICIENT_BUFFER.
        let mut needed = 0u32;
        // SAFETY: no buffer, length 0; `needed` is a stack out-parameter.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        if (needed as usize) < std::mem::size_of::<TOKEN_USER>() {
            return Err(io::Error::other(format!(
                "sizing the token user information: {}",
                io::Error::last_os_error()
            )));
        }
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: `buf` holds at least `needed` writable bytes for the call.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|e| io::Error::other(format!("reading the token user information: {e}")))?;
        Ok(Self { buf })
    }

    fn sid(&self) -> PSID {
        // SAFETY: `buf` starts with the `TOKEN_USER` GetTokenInformation
        // wrote, aligned for it and at least its size (checked when sizing).
        unsafe { (*self.buf.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// Whether both are one user.
    #[cfg(feature = "single-instance")]
    pub(crate) fn is(&self, other: &Self) -> bool {
        // SAFETY: both SIDs point into buffers the system filled, alive for
        // the call.
        unsafe { EqualSid(self.sid(), other.sid()) }.is_ok()
    }

    /// The SID in its `S-1-5-...` string form.
    pub(crate) fn sid_string(&self) -> io::Result<String> {
        let mut sid_str = PWSTR::null();
        // SAFETY: the SID points into `self.buf`; `sid_str` is an
        // out-parameter freed with LocalFree below.
        unsafe { ConvertSidToStringSidW(self.sid(), &mut sid_str) }
            .map_err(|e| io::Error::other(format!("formatting the user SID: {e}")))?;
        // SAFETY: a NUL-terminated string the call allocated, freed once read.
        let owned = unsafe {
            let owned = sid_str.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(sid_str.0.cast())));
            owned
        };
        if owned.is_empty() {
            return Err(io::Error::other(
                "the user SID formatted to an empty string",
            ));
        }
        Ok(owned)
    }
}

/// The current process token's user SID, in `S-1-5-...` string form.
pub(crate) fn current_user_sid_string() -> io::Result<String> {
    ProcessUser::current()?.sid_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_user_sid_is_resolvable() {
        // The DACL is built from this; if it cannot be read, `bind` must fail
        // rather than fall back to the permissive default descriptor.
        let sid = current_user_sid_string().expect("current user SID");
        assert!(sid.starts_with("S-1-"), "{sid}");
    }

    #[test]
    fn the_descriptor_is_owner_only_and_protected() {
        let sec = OwnerOnlySecurity::current_user().expect("descriptor");
        assert!(!sec.descriptor.is_invalid());
        let attrs = sec.attributes();
        assert_eq!(
            attrs.nLength as usize,
            std::mem::size_of::<SECURITY_ATTRIBUTES>()
        );
        assert!(!attrs.lpSecurityDescriptor.is_null());
        assert!(
            !attrs.bInheritHandle.as_bool(),
            "handles must not be inheritable"
        );
    }

    #[cfg(feature = "single-instance")]
    #[test]
    fn this_process_runs_as_the_current_user() {
        let me = ProcessUser::current().unwrap();
        let again = ProcessUser::of_process(std::process::id()).unwrap();
        assert!(me.is(&again));
        assert_eq!(me.sid_string().unwrap(), again.sid_string().unwrap());
    }
}
