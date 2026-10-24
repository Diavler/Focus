//! The `Autostart` port for Windows: a value under the current user's `Run` key, which Windows
//! starts at login. HKCU needs no admin.

use std::io;
use std::path::Path;

use focus_core::ports::{Autostart, AutostartError};
use widestring::U16CString;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
};

use crate::Mode;

/// The key Windows reads at login, under HKEY_CURRENT_USER.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Starts `focus-daemon --at-login` from the install folder when the user logs in.
#[derive(Debug, Clone)]
pub struct WindowsAutostart {
    mode: Mode,
    value_name: String,
    command: String,
}

impl WindowsAutostart {
    /// Registers the daemon in `install` under the value name `Focus`.
    pub fn new(mode: Mode, install: &Path) -> Self {
        let daemon = install.join("focus-daemon.exe");
        Self {
            mode,
            value_name: "Focus".to_owned(),
            command: format!("\"{}\" --at-login", daemon.display()),
        }
    }

    /// Uses another value name, so tests don't touch the real entry.
    pub fn with_value_name(mut self, value_name: &str) -> Self {
        value_name.clone_into(&mut self.value_name);
        self
    }

    /// The command line registered to run at login.
    pub fn command(&self) -> &str {
        &self.command
    }

    fn write(&self) -> io::Result<()> {
        let key = wide(RUN_KEY)?;
        let name = wide(&self.value_name)?;
        let data = wide(&self.command)?;
        let bytes = u32::try_from(data.as_slice_with_nul().len() * 2)
            .map_err(|_| io::Error::other("command too long"))?;
        // SAFETY: all strings are NUL-terminated UTF-16 that live through the call, and `bytes`
        // is the data's size including its terminator, as REG_SZ requires.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        check(status)
    }

    fn remove(&self) -> io::Result<()> {
        let key = wide(RUN_KEY)?;
        let name = wide(&self.value_name)?;
        // SAFETY: both strings are NUL-terminated UTF-16 that live through the call.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }
}

impl Autostart for WindowsAutostart {
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        if self.mode == Mode::DryRun {
            let what = if enabled { "register" } else { "remove" };
            tracing::info!(
                "dry run: would {what} HKCU\\{RUN_KEY}\\{} = {}",
                self.value_name,
                self.command
            );
            return Ok(());
        }
        let result = if enabled { self.write() } else { self.remove() };
        result.map_err(|error| AutostartError::Failed {
            reason: error.to_string(),
        })
    }
}

fn wide(text: &str) -> io::Result<U16CString> {
    U16CString::from_str(text).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

fn check(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(i32::MAX),
        ))
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::autostart_contract;
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};

    use super::*;

    /// The registered command for `autostart`'s value, or `None` if there is none.
    fn registered(autostart: &WindowsAutostart) -> Option<String> {
        let key = wide(RUN_KEY).unwrap();
        let name = wide(&autostart.value_name).unwrap();
        let mut buffer = [0_u16; 1024];
        let mut bytes = u32::try_from(buffer.len() * 2).unwrap();
        // SAFETY: the buffer holds `bytes` bytes; the strings are NUL-terminated.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        (status == 0).then(|| crate::processes::wide_to_string(&buffer))
    }

    /// An entry only this test uses (tests run in parallel in one process).
    fn test_entry(mode: Mode, test: &str) -> WindowsAutostart {
        WindowsAutostart::new(mode, Path::new(r"C:\Focus"))
            .with_value_name(&format!("FocusTest-{}-{test}", std::process::id()))
    }

    #[test]
    fn registers_and_removes_a_run_entry() {
        let autostart = test_entry(Mode::Live, "register");
        autostart.set_enabled(true).unwrap();
        assert_eq!(
            registered(&autostart).as_deref(),
            Some(r#""C:\Focus\focus-daemon.exe" --at-login"#)
        );
        autostart.set_enabled(false).unwrap();
        assert_eq!(registered(&autostart), None);
    }

    #[test]
    fn passes_the_contract_on_a_test_entry_and_in_dry_run() {
        let live = test_entry(Mode::Live, "contract");
        autostart_contract(&live);
        assert_eq!(registered(&live), None, "the contract ends switched off");
        autostart_contract(&test_entry(Mode::DryRun, "contract-dry"));
    }

    #[test]
    fn dry_run_changes_nothing() {
        let autostart = test_entry(Mode::DryRun, "dry");
        autostart.set_enabled(true).unwrap();
        assert_eq!(registered(&autostart), None);
    }
}
