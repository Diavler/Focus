//! Starting one Focus executable from another.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The folder holding the running executable, which holds the other Focus executables too.
pub fn install_folder() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("the executable's path has no folder"))
}

/// Starts `<install>/<name>` (plus `.exe` on Windows) with `args`, detached: no console window,
/// no inherited input or output, and it keeps running after this process exits.
pub fn spawn_sibling(install: &Path, name: &str, args: &[&str]) -> io::Result<()> {
    let program = install.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Don't open a console window for a console-subsystem program.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // The child is deliberately not waited for: it's a long-lived sibling, not a helper.
    command.spawn().map(|_detached| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_folder_holds_this_executable() {
        let folder = install_folder().unwrap();
        assert!(std::env::current_exe().unwrap().starts_with(folder));
    }

    #[test]
    fn a_missing_sibling_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(spawn_sibling(dir.path(), "no-such-focus-program", &[]).is_err());
    }
}
