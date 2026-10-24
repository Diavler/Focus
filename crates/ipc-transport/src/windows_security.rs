//! Windows only: the daemon's pipe name, a pipe security descriptor that admits only the current
//! user, and refusing a pipe served by another user (via `win-user`). Named pipe names are machine-wide, so without these another user could create
//! `focus-daemon-<you>` first and pose as your daemon (ADR 0004).

use std::io;

use interprocess::local_socket::tokio::Stream as LocalSocketStream;
use interprocess::local_socket::traits::StreamCommon;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;
use win_user::{current_user_sid, process_user_sid};

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

/// A security descriptor granting the pipe to SYSTEM and the current user only. `P` stops
/// inherited entries, so nobody else, including other users, can open or create instances of it.
pub(crate) fn current_user_only() -> io::Result<SecurityDescriptor> {
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", current_user_sid()?);
    let wide = U16CString::from_str(&sddl)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    SecurityDescriptor::deserialize(&wide)
}

#[cfg(test)]
mod tests {
    use super::*;

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
