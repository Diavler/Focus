//! The `GlobalHotkey` port on Windows: `RegisterHotKey` on a thread with its own message loop,
//! which gets `WM_HOTKEY` and calls back. The process that receives a hotkey may bring its own
//! window to the front, which is why the UI (not the daemon) listens.

use std::ptr::null_mut;
use std::sync::mpsc;
use std::thread::JoinHandle;

use focus_core::Hotkey;
use focus_core::ports::{GlobalHotkey, HotkeyError, HotkeyGuard};
use windows_sys::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, GetLastError};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_P, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_HOTKEY, WM_QUIT, WM_USER,
};

/// The id the shortcut is registered under on its thread.
const HOTKEY_ID: i32 = 1;

/// System-wide shortcuts with `RegisterHotKey`, for the current user's session.
#[derive(Debug, Default)]
pub struct WindowsHotkey;

impl GlobalHotkey for WindowsHotkey {
    fn listen(
        &self,
        hotkey: Hotkey,
        pressed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<HotkeyGuard, HotkeyError> {
        Ok(match register(hotkey, pressed)? {
            Some(registration) => HotkeyGuard::new(registration),
            None => HotkeyGuard::none(),
        })
    }
}

/// The modifiers and virtual key of a shortcut; `None` for Off.
fn keys(hotkey: Hotkey) -> Option<(u32, u32)> {
    let (modifiers, key) = match hotkey {
        Hotkey::Off => return None,
        Hotkey::CtrlAltSpace => (MOD_CONTROL | MOD_ALT, VK_SPACE),
        Hotkey::CtrlShiftSpace => (MOD_CONTROL | MOD_SHIFT, VK_SPACE),
        Hotkey::CtrlAltP => (MOD_CONTROL | MOD_ALT, VK_P),
    };
    Some((modifiers, u32::from(key)))
}

/// A registered shortcut: its thread, stopped (and the shortcut unregistered) when dropped.
struct Registration {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: posts a message to the thread this registration started; if it has already
        // exited the call just fails, which is fine.
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Registers `hotkey` on a new thread that calls `pressed` for each press. `None` for Off.
fn register(
    hotkey: Hotkey,
    pressed: Box<dyn Fn() + Send + Sync>,
) -> Result<Option<Registration>, HotkeyError> {
    let Some((modifiers, key)) = keys(hotkey) else {
        return Ok(None);
    };
    let (ready, started) = mpsc::sync_channel::<Result<u32, u32>>(1);
    let thread = std::thread::Builder::new()
        .name("focus-hotkey".into())
        .spawn(move || listen_on_this_thread(modifiers, key, &ready, &*pressed))
        .map_err(|error| HotkeyError::Failed {
            reason: error.to_string(),
        })?;
    match started.recv() {
        Ok(Ok(thread_id)) => Ok(Some(Registration {
            thread_id,
            thread: Some(thread),
        })),
        Ok(Err(code)) => {
            let _ = thread.join();
            Err(if code == ERROR_HOTKEY_ALREADY_REGISTERED {
                HotkeyError::Taken { hotkey }
            } else {
                HotkeyError::Failed {
                    reason: format!("Windows error {code}"),
                }
            })
        }
        Err(_) => Err(HotkeyError::Failed {
            reason: "the shortcut thread stopped before registering".into(),
        }),
    }
}

/// The shortcut thread: registers, reports the thread id (or the error), then waits for presses
/// until `WM_QUIT`.
fn listen_on_this_thread(
    modifiers: u32,
    key: u32,
    ready: &mpsc::SyncSender<Result<u32, u32>>,
    pressed: &(dyn Fn() + Send + Sync),
) {
    // SAFETY: Win32 calls on this thread's own message queue. `msg` is a plain struct on the
    // stack, valid for every call that writes to it; a null window means "this thread".
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        // Creates the thread's message queue before anyone can post WM_QUIT to it.
        PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
        if RegisterHotKey(null_mut(), HOTKEY_ID, modifiers | MOD_NOREPEAT, key) == 0 {
            let _ = ready.send(Err(GetLastError()));
            return;
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                pressed();
            }
        }
        UnregisterHotKey(null_mut(), HOTKEY_ID);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use focus_core::testing::global_hotkey_contract;

    use super::*;

    // Ctrl+Alt+P: not the default, so a Focus window running on the same machine (which holds
    // Ctrl+Alt+Space) doesn't make these tests skip.
    const TEST_HOTKEY: Hotkey = Hotkey::CtrlAltP;

    /// The tests share one system-wide shortcut, so they take turns instead of seeing it taken.
    static SHORTCUT: Mutex<()> = Mutex::new(());

    #[test]
    fn passes_the_contract() {
        let _turn = SHORTCUT.lock().unwrap_or_else(PoisonError::into_inner);
        global_hotkey_contract(&WindowsHotkey, TEST_HOTKEY);
    }

    #[test]
    fn calls_back_on_a_press() {
        let _turn = SHORTCUT.lock().unwrap_or_else(PoisonError::into_inner);
        let (sender, presses) = mpsc::channel();
        let callback = Box::new(move || {
            let _ = sender.send(());
        });
        let registration = match register(TEST_HOTKEY, callback) {
            Ok(Some(registration)) => registration,
            Ok(None) => unreachable!("not Off"),
            // Another app holds it on this machine; the contract test covers what it can.
            Err(HotkeyError::Taken { .. }) => return,
            Err(error) => panic!("{error}"),
        };
        // A real press would type on the desktop, so post the message Windows would send.
        // SAFETY: posting to the thread `register` just started.
        unsafe {
            PostThreadMessageW(registration.thread_id, WM_HOTKEY, HOTKEY_ID as usize, 0);
        }
        assert!(presses.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(registration);
    }
}
