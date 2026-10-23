//! Where the daemon listens, and binding to it as the only daemon for this user.

use std::fmt;
use std::io;
use std::path::PathBuf;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions, Name};
use thiserror::Error;

use crate::connection::Connection;

/// A local socket address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A socket file. Its directory's permissions decide who can connect.
    Path(PathBuf),
    /// A name in the OS's socket namespace: a named pipe on Windows, an abstract socket on Linux.
    Namespaced(String),
}

impl Endpoint {
    /// The current user's daemon endpoint.
    ///
    /// With `XDG_RUNTIME_DIR` set (Linux desktops), a socket file in that directory, which only the
    /// user can open. Otherwise (Windows), a named pipe whose name includes the user name, so users
    /// on the same machine don't collide.
    pub fn for_current_user() -> Self {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
            return Self::Path(PathBuf::from(dir).join("focus-daemon.sock"));
        }
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default();
        let user: String = user
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        Self::Namespaced(format!("focus-daemon-{user}"))
    }

    /// The OS-level socket name.
    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            Self::Path(path) => path.as_path().to_fs_name::<GenericFilePath>(),
            Self::Namespaced(name) => name.as_str().to_ns_name::<GenericNamespaced>(),
        }
    }

    /// Opens a connection to whatever listens here.
    pub(crate) async fn connect(&self) -> io::Result<Connection> {
        let stream = LocalSocketStream::connect(self.name()?).await?;
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
/// A socket file left behind by a crashed daemon is replaced. On Windows the first pipe instance
/// is exclusive, so a second daemon fails to bind even if it races past the connect check.
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
