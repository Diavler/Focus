//! The `ForegroundApp` port for Windows.

use focus_core::ports::{EnforceError, ForegroundApp};
use win_user::OwnedHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::processes::{app_name, wide_to_string};

/// Reports the app owning the foreground window.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsForeground;

impl ForegroundApp for WindowsForeground {
    fn foreground_app(&self) -> Result<Option<String>, EnforceError> {
        // SAFETY: plain Win32 call; null means no foreground window (e.g. the lock screen).
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return Ok(None);
        }
        let mut pid = 0_u32;
        // SAFETY: `window` is a window handle and the out-pointer is valid.
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == 0 {
            return Ok(None);
        }
        // SAFETY: plain Win32 call; null means failure (e.g. a protected process), reported as
        // "unknown" rather than an error, since the window is still there.
        let Some(process) = (unsafe {
            OwnedHandle::from_raw(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))
        }) else {
            return Ok(None);
        };
        let mut path = [0_u16; 1024];
        let mut length = u32::try_from(path.len()).unwrap_or(u32::MAX);
        // SAFETY: `path` holds `length` UTF-16 units and the handle has query access.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                process.raw(),
                PROCESS_NAME_WIN32,
                path.as_mut_ptr(),
                &mut length,
            )
        };
        if ok == 0 {
            return Ok(None);
        }
        let name = app_name(&wide_to_string(&path));
        Ok((!name.is_empty()).then_some(name))
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::foreground_app_contract;

    use super::*;

    #[test]
    fn passes_the_contract() {
        // CI machines may have no foreground window; the contract accepts `None`.
        foreground_app_contract(&WindowsForeground);
    }
}
