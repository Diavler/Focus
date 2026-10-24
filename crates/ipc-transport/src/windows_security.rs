//! Windows only: which user runs a process, and a pipe security descriptor that admits only the
//! current user. Named pipe names are machine-wide, so without these another user could create
//! `focus-daemon-<you>` first and pose as your daemon (ADR 0004).

use std::io;
use std::ptr;

use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::{U16CStr, U16CString};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Closes a kernel handle when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful Win32 call and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// The SID (`S-1-5-...`) of the user this process runs as.
pub(crate) fn current_user_sid() -> io::Result<String> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid and needs no closing.
    token_user_sid(unsafe { GetCurrentProcess() })
}

/// The SID of the user running process `pid`. Fails (access denied) for most processes of other
/// users, which callers treat the same as a different SID.
pub(crate) fn process_user_sid(pid: u32) -> io::Result<String> {
    // SAFETY: plain Win32 call; a null handle means failure.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = Handle(process);
    token_user_sid(process.0)
}

/// A security descriptor granting the pipe to SYSTEM and the current user only. `P` stops
/// inherited entries, so nobody else, including other users, can open or create instances of it.
pub(crate) fn current_user_only() -> io::Result<SecurityDescriptor> {
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", current_user_sid()?);
    let wide = U16CString::from_str(&sddl)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    SecurityDescriptor::deserialize(&wide)
}

fn token_user_sid(process: HANDLE) -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `token` is a valid out-pointer for the new handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);
    let mut needed = 0_u32;
    // SAFETY: a null buffer of length 0 asks only for the size; this call "fails" by design.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 elements keep the buffer aligned for TOKEN_USER, which holds pointers.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `needed` bytes and suitably aligned.
    let filled = unsafe {
        GetTokenInformation(
            token.0,
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
    }

    #[test]
    fn another_users_or_missing_process_is_an_error_or_different() {
        // PID 4 is the System process: a different user (or not openable at all).
        let me = current_user_sid().unwrap();
        assert!(process_user_sid(4).map_or(true, |sid| sid != me));
    }

    #[test]
    fn builds_the_descriptor() {
        current_user_only().unwrap();
    }
}
