//! Closing a process: politely through its windows, or by terminating it.

use std::io;

use win_user::OwnedHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    WM_CLOSE,
};
use windows_sys::core::BOOL;

/// The process's visible top-level windows, the ones a user would close.
pub(crate) fn top_level_windows(pid: u32) -> Vec<HWND> {
    struct Search {
        pid: u32,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn visit(window: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `&mut Search` passed to EnumWindows below, alive for the call.
        let search = unsafe { &mut *(state as *mut Search) };
        let mut owner_pid = 0_u32;
        // SAFETY: `window` comes from EnumWindows; the out-pointer is valid.
        unsafe { GetWindowThreadProcessId(window, &mut owner_pid) };
        // SAFETY: plain queries on a window handle EnumWindows just gave us.
        let user_window =
            unsafe { IsWindowVisible(window) != 0 && GetWindow(window, GW_OWNER).is_null() };
        if owner_pid == search.pid && user_window {
            search.found.push(window);
        }
        1
    }

    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    // SAFETY: `visit` matches WNDENUMPROC and only uses `search` during this call.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    search.found
}

/// Asks each window to close, as clicking its close button would.
pub(crate) fn ask_to_close(windows: &[HWND]) {
    for &window in windows {
        // SAFETY: posting a message to a window handle is safe even if the window has gone.
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    }
}

/// Ends the process immediately.
pub(crate) fn terminate(pid: u32) -> io::Result<()> {
    // SAFETY: plain Win32 call; a null handle means failure.
    let process = unsafe { OwnedHandle::from_raw(OpenProcess(PROCESS_TERMINATE, 0, pid)) }
        .ok_or_else(io::Error::last_os_error)?;
    // SAFETY: the handle has PROCESS_TERMINATE access.
    if unsafe { TerminateProcess(process.raw(), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
