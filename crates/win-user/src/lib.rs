//! Windows only: which user runs a process. Used to keep Focus's named pipe to its own user
//! (`ipc-transport`, ADR 0004) and to close only the current user's apps (`enforce-windows`).
//! Empty on other platforms.

#![cfg(windows)]

use std::io;
use std::ptr;
use std::sync::OnceLock;

use widestring::U16CStr;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// A kernel handle, closed when dropped.
#[derive(Debug)]
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Takes ownership of `handle`, or `None` if it is null (a failed Win32 call).
    ///
    /// # Safety
    /// `handle` must be a real handle (not a pseudo-handle) that nothing else will close.
    pub unsafe fn from_raw(handle: HANDLE) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }

    /// The raw handle, valid while `self` lives.
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// The SID (`S-1-5-...`) of the user this process runs as.
///
/// Read once and kept for the life of the process: a process can't change user, and callers ask
/// often. A cached OS fact, not configuration or shared state.
pub fn current_user_sid() -> io::Result<&'static str> {
    static SID: OnceLock<String> = OnceLock::new();
    if let Some(sid) = SID.get() {
        return Ok(sid);
    }
    // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid and needs no closing.
    let sid = token_user_sid(unsafe { GetCurrentProcess() })?;
    Ok(SID.get_or_init(|| sid))
}

/// The SID of the user running process `pid`. Fails (access denied) for most processes of other
/// users and for elevated processes; callers treat that as "not ours".
pub fn process_user_sid(pid: u32) -> io::Result<String> {
    // SAFETY: plain Win32 call; a null handle means failure.
    let process =
        unsafe { OwnedHandle::from_raw(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)) }
            .ok_or_else(io::Error::last_os_error)?;
    token_user_sid(process.raw())
}

/// Whether process `pid` runs as the current user. Errors count as "no".
pub fn is_current_user_process(pid: u32) -> bool {
    match (process_user_sid(pid), current_user_sid()) {
        (Ok(sid), Ok(me)) => sid.eq_ignore_ascii_case(me),
        _ => false,
    }
}

fn token_user_sid(process: HANDLE) -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `token` is a valid out-pointer for the new handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken succeeded, so `token` is a real handle we now own.
    let token = unsafe { OwnedHandle::from_raw(token) }.ok_or_else(io::Error::last_os_error)?;
    let mut needed = 0_u32;
    // SAFETY: a null buffer of length 0 asks only for the size; this call "fails" by design.
    unsafe { GetTokenInformation(token.raw(), TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 elements keep the buffer aligned for TOKEN_USER, which holds pointers.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `needed` bytes and suitably aligned.
    let filled = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if filled == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID points into the buffer,
    // which outlives this borrow.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    sid_to_string(user.User.Sid)
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is valid for this call and `text` is a valid out-pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success `text` is a NUL-terminated UTF-16 string allocated with LocalAlloc,
    // read once here and then freed.
    let sid = unsafe { U16CStr::from_ptr_str(text) }.to_string_lossy();
    // SAFETY: frees the LocalAlloc'd string exactly once.
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_runs_as_the_current_user() {
        let me = current_user_sid().unwrap();
        assert!(me.starts_with("S-1-5-"), "{me}");
        assert_eq!(process_user_sid(std::process::id()).unwrap(), me);
        assert_eq!(current_user_sid().unwrap(), me, "cached value is stable");
        assert!(is_current_user_process(std::process::id()));
    }

    #[test]
    fn system_processes_are_not_ours() {
        // PID 4 is the System process.
        assert!(!is_current_user_process(4));
    }
}
