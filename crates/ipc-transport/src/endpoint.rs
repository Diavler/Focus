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
    /// listener.
    pub(crate) async fn connect(&self) -> io::Result<Connection> {
        let options = ConnectOptions::new()
            .name(self.name()?)
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT));
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, options.connect_tokio())
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "the daemon didn't accept in time")
            })??;
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
pub async fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
    if endpoint.connect().await.is_ok() {
        return Err(BindError::AlreadyRunning(endpoint.clone()));
    }
    let io_error = |source| BindError::Io {
        endpoint: endpoint.clone(),
        source,
    };
    let name = endpoint.name().map_err(io_error)?;
    match ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_tokio()
    {
        Ok(inner) => Ok(Listener {
            inner,
            endpoint: endpoint.clone(),
        }),
        Err(_) if endpoint.connect().await.is_ok() => {
            Err(BindError::AlreadyRunning(endpoint.clone()))
        }
        Err(source) => Err(io_error(source)),
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
