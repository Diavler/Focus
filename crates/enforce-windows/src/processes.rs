//! Listing processes and naming apps.

use std::io;
use std::path::Path;

use focus_core::same_app;
use win_user::OwnedHandle;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

/// Apps Focus never closes, whatever the config says: itself and the parts of the Windows shell
/// a user's session can't do without.
const PROTECTED: [&str; 12] = [
    "focus-daemon",
    "focus-ui",
    "focus-nmh",
    "explorer",
    "dwm",
    "sihost",
    "ctfmon",
    "taskhostw",
    "runtimebroker",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "searchhost",
];

/// One running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Process {
    pub(crate) pid: u32,
    /// The executable's name without `.exe`, e.g. `chrome`.
    pub(crate) app: String,
}

/// Every process on the machine, except this one.
pub(crate) fn all_processes() -> io::Result<Vec<Process>> {
    // SAFETY: plain Win32 call; returns INVALID_HANDLE_VALUE (-1) on failure, checked below.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the snapshot handle is real and owned here.
    let snapshot = unsafe { OwnedHandle::from_raw(raw) }.ok_or_else(io::Error::last_os_error)?;
    let own_pid = std::process::id();
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut processes = Vec::new();
    // SAFETY: `entry` is a valid PROCESSENTRY32W with dwSize set, as the API requires.
    let mut more = unsafe { Process32FirstW(snapshot.raw(), &mut entry) } != 0;
    while more {
        if entry.th32ProcessID != own_pid {
            processes.push(Process {
                pid: entry.th32ProcessID,
                app: app_name(&wide_to_string(&entry.szExeFile)),
            });
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.raw(), &mut entry) } != 0;
    }
    Ok(processes)
}

/// Processes of the current user whose app is one of `apps`, never protected ones.
pub(crate) fn own_matching(apps: &[String]) -> io::Result<Vec<Process>> {
    Ok(all_processes()?
        .into_iter()
        .filter(|process| apps.iter().any(|app| same_app(app, &process.app)))
        .filter(|process| !is_protected(&process.app))
        .filter(|process| win_user::is_current_user_process(process.pid))
        .collect())
}

/// Whether Focus must never close `app`.
pub(crate) fn is_protected(app: &str) -> bool {
    PROTECTED.iter().any(|protected| same_app(protected, app))
}

/// `C:\Path\Chrome.exe` or `Chrome.exe` → `Chrome`.
pub(crate) fn app_name(executable: &str) -> String {
    let file = Path::new(executable).file_name().map_or_else(
        || executable.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    match file.len().checked_sub(4) {
        Some(stem) if file[stem..].eq_ignore_ascii_case(".exe") => file[..stem].to_owned(),
        _ => file,
    }
}

/// A NUL-terminated UTF-16 buffer as a string.
pub(crate) fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_apps_from_paths() {
        assert_eq!(app_name(r"C:\Program Files\Google\Chrome.EXE"), "Chrome");
        assert_eq!(app_name("discord.exe"), "discord");
        assert_eq!(app_name("noext"), "noext");
    }

    #[test]
    fn protects_focus_and_the_shell() {
        assert!(is_protected("Explorer.EXE"));
        assert!(is_protected("focus-daemon"));
        assert!(!is_protected("discord"));
    }

    #[test]
    fn lists_processes_but_not_itself() {
        let processes = all_processes().unwrap();
        assert!(processes.len() > 5);
        assert!(
            processes
                .iter()
                .all(|process| process.pid != std::process::id())
        );
        assert!(
            processes
                .iter()
                .any(|process| process.app.eq_ignore_ascii_case("explorer")
                    || process.app.eq_ignore_ascii_case("svchost"))
        );
    }
}
