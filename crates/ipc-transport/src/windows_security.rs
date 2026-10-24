//! Windows only: which user runs a process, and a pipe security descriptor that admits only the
//! current user. Named pipe names are machine-wide, so without these another user could create
//! `focus-daemon-<you>` first and pose as your daemon (ADR 0004).

use std::io;
use std::ptr;
use std::sync::OnceLock;

use interprocess::local_socket::tokio::Stream as LocalSocketStream;
use interprocess::local_socket::traits::StreamCommon;
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
///
/// Read once and kept for the life of the process: a process can't change user, and the UI and
/// clients connect often. A cached OS fact, not configuration or shared state.
pub(crate) fn current_user_sid() -> io::Result<&'static str> {
    static SID: OnceLock<String> = OnceLock::new();
    if let Some(sid) = SID.get() {
        return Ok(sid);
    }
    // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid and needs no closing.
    let sid = token_user_sid(unsafe { GetCurrentProcess() })?;
    Ok(SID.get_or_init(|| sid))
}

/// The daemon's pipe name, `focus-daemon-<user SID>`: unique per account, unlike user names,
/// which can repeat across domains or be unset. Falls back to the user name if the SID can't be
/// read.
pub(crate) fn pipe_name() -> String {
    match current_user_sid() {
        Ok(sid) => format!("focus-daemon-{sid}"),
        Err(_) => {
            let user = std::env::var("USERNAME").unwrap_or_default();
            // Pipe names may contain anything but a backslash.
            format!("focus-daemon-{}", user.replace('\\', "_"))
        }
    }
}

/// Why a pipe was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The server runs as another user.
    OtherUser,
    /// The server's user couldn't be read, e.g. it runs elevated (Focus never needs admin).
    Unverifiable,
}

/// Accepts a pipe server only if it runs as `me`. `server` is the server process's user SID, or
/// the error from trying to read it.
pub(crate) fn check_server(server: &io::Result<String>, me: &str) -> Result<(), Refusal> {
    match server {
        Ok(sid) if sid.eq_ignore_ascii_case(me) => Ok(()),
        Ok(_) => Err(Refusal::OtherUser),
        Err(_) => Err(Refusal::Unverifiable),
    }
}

/// Refuses, with `PermissionDenied`, a pipe whose server isn't this user's process.
///
/// The server is identified by its process ID (`GetNamedPipeServerProcessId`), which stays
/// meaningful while the connection is open unless the daemon died after leaking its pipe handle
/// to another process, which Focus never does.
pub(crate) fn ensure_same_user(stream: &LocalSocketStream) -> io::Result<()> {
    let me = current_user_sid()?;
    let server = match stream.peer_creds()?.pid() {
        Some(pid) => process_user_sid(pid),
        None => Err(io::Error::other("no server process ID")),
    };
    check_server(&server, me).map_err(|refusal| {
        let message = match refusal {
            Refusal::OtherUser => "the Focus daemon endpoint belongs to another user's process",
            Refusal::Unverifiable => {
                "can't check who runs the Focus daemon (is it running as administrator? \
                 Focus must run without admin)"
            }
        };
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    })
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
        assert_eq!(current_user_sid().unwrap(), me, "cached value is stable");
    }

    #[test]
    fn builds_the_descriptor() {
        current_user_only().unwrap();
    }

    #[test]
    fn pipe_name_is_per_account() {
        let me = current_user_sid().unwrap();
        assert_eq!(pipe_name(), format!("focus-daemon-{me}"));
    }

    #[test]
    fn accepts_only_the_same_user() {
        let me = "S-1-5-21-1-2-3-1001";
        assert_eq!(check_server(&Ok(me.to_owned()), me), Ok(()));
        assert_eq!(check_server(&Ok(me.to_lowercase()), me), Ok(()));
        assert_eq!(
            check_server(&Ok("S-1-5-21-1-2-3-1002".to_owned()), me),
            Err(Refusal::OtherUser)
        );
        assert_eq!(
            check_server(&Err(io::Error::other("access denied")), me),
            Err(Refusal::Unverifiable)
        );
    }
}
