//! Where the daemon listens, and binding to it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use interprocess::ConnectWaitMode;
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{
    ConnectOptions, GenericFilePath, GenericNamespaced, ListenerOptions, Name,
};
use thiserror::Error;

use crate::connection::Connection;

/// What a client reports when the endpoint is held by a process of another user.
#[cfg(windows)]
const OTHER_USER: &str = "the Focus daemon endpoint belongs to another user's process";

/// How long a connect may wait for a busy daemon before giving up.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// A local socket address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A socket file. Its folder's permissions decide who can connect.
    Path(PathBuf),
    /// A name in the OS's socket namespace: a named pipe on Windows, an abstract socket on Linux.
    /// Linux abstract sockets have no permissions, so only tests use this there.
    Namespaced(String),
}

impl Endpoint {
    /// The endpoint of the daemon installed in `install` (the folder holding the executables).
    ///
    /// On Windows, the named pipe `focus-daemon-<user>`. Elsewhere, the socket file
    /// `<install>/data/daemon.sock`, inside the portable install like the rest of Focus's files
    /// and independent of environment variables, so every process of the install agrees on it.
    pub fn for_install(install: &Path) -> Self {
        if std::env::consts::FAMILY == "windows" {
            let user = std::env::var("USERNAME").unwrap_or_default();
            // Pipe names may contain anything but a backslash.
            Self::Namespaced(format!("focus-daemon-{}", user.replace('\\', "_")))
        } else {
            Self::Path(install.join("data").join("daemon.sock"))
        }
    }

    /// The OS-level socket name.
    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            Self::Path(path) => path.as_path().to_fs_name::<GenericFilePath>(),
            Self::Namespaced(name) => name.as_str().to_ns_name::<GenericNamespaced>(),
        }
    }

    /// Opens a connection to whatever listens here, waiting at most [`CONNECT_TIMEOUT`] for a busy
    /// listener. On Windows, a pipe served by another user's process is refused with
    /// `PermissionDenied`: pipe names are machine-wide, so another user could have created ours.
    pub(crate) async fn connect(&self) -> io::Result<Connection> {
        let options = ConnectOptions::new()
            .name(self.name()?)
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT));
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, options.connect_tokio())
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "the daemon didn't accept in time")
            })??;
        #[cfg(windows)]
        ensure_same_user(&stream)?;
        Ok(Connection::new(stream))
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => write!(f, "{}", path.display()),
            Self::Namespaced(name) => write!(f, "@{name}"),
        }
    }
}

/// A bound endpoint that accepts connections.
#[derive(Debug)]
pub struct Listener {
    inner: LocalSocketListener,
    endpoint: Endpoint,
}

impl Listener {
    /// Waits for the next client.
    pub async fn accept(&self) -> io::Result<Connection> {
        self.inner.accept().await.map(Connection::new)
    }

    /// Where this listener is bound.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

/// Why binding failed.
#[derive(Debug, Error)]
pub enum BindError {
    /// Another daemon already answers on this endpoint.
    #[error("a Focus daemon is already running at {0}")]
    AlreadyRunning(Endpoint),
    /// A process of another user holds this endpoint (Windows pipe squatting).
    #[error("{0} is held by another user's process; Focus won't use it")]
    TakenByAnotherUser(Endpoint),
    /// The OS refused.
    #[error("couldn't listen at {endpoint}: {source}")]
    Io {
        /// The endpoint.
        endpoint: Endpoint,
        /// The OS error.
        source: io::Error,
    },
}

/// Binds `endpoint`, refusing if another daemon already answers there.
///
/// A socket file nobody answers on is treated as left behind by a crash and replaced. Two
/// processes binding at the same moment could both see it as stale, so callers must make sure
/// only one of them gets here: the daemon holds an exclusive lock on `data/daemon.lock` first.
/// (On Windows the first pipe instance is exclusive anyway.)
///
/// On Windows the pipe only admits the current user (and SYSTEM), and a pipe already created by
/// another user's process is reported as [`BindError::TakenByAnotherUser`].
pub async fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
    if let Some(error) = held_by(endpoint).await {
        return Err(error);
    }
    let io_error = |source| BindError::Io {
        endpoint: endpoint.clone(),
        source,
    };
    let name = endpoint.name().map_err(io_error)?;
    let options = ListenerOptions::new().name(name).try_overwrite(true);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        options.security_descriptor(crate::windows_security::current_user_only().map_err(io_error)?)
    };
    match options.create_tokio() {
        Ok(inner) => Ok(Listener {
            inner,
            endpoint: endpoint.clone(),
        }),
        Err(source) => Err(held_by(endpoint).await.unwrap_or_else(|| io_error(source))),
    }
}

/// Who already listens at `endpoint`: our own daemon, another user's process, or nobody (`None`).
async fn held_by(endpoint: &Endpoint) -> Option<BindError> {
    match endpoint.connect().await {
        Ok(_) => Some(BindError::AlreadyRunning(endpoint.clone())),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            Some(BindError::TakenByAnotherUser(endpoint.clone()))
        }
        Err(_) => None,
    }
}

/// Refuses a pipe whose server runs as another user (or can't be checked).
#[cfg(windows)]
fn ensure_same_user(stream: &LocalSocketStream) -> io::Result<()> {
    use crate::windows_security::{current_user_sid, process_user_sid};
    let other_user = || io::Error::new(io::ErrorKind::PermissionDenied, OTHER_USER);
    let pid = stream.peer_creds()?.pid().ok_or_else(other_user)?;
    match process_user_sid(pid) {
        Ok(sid) if sid == current_user_sid()? => Ok(()),
        _ => Err(other_user()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_endpoint_is_a_named_pipe_on_windows_and_a_data_socket_elsewhere() {
        let install = Path::new("Focus");
        match Endpoint::for_install(install) {
            Endpoint::Namespaced(name) => {
                assert_eq!(std::env::consts::FAMILY, "windows");
                assert!(name.starts_with("focus-daemon-"), "{name}");
                assert!(!name.contains('\\'), "{name}");
            }
            Endpoint::Path(path) => {
                assert_eq!(path, install.join("data").join("daemon.sock"));
            }
        }
    }
}
