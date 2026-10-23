//! One open connection, carrying newline-terminated lines in both directions.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use interprocess::local_socket::tokio::prelude::*;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, ReadBuf};

/// The longest line accepted, in bytes. Larger lines close the connection, so a misbehaving
/// client can't make the daemon buffer without limit.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// A connection that sends and receives lines.
#[derive(Debug)]
pub struct Connection {
    reader: BufReader<SharedStream>,
    stream: Arc<LocalSocketStream>,
}

/// Reads from a stream that is also written through another handle. `&Stream` implements
/// `AsyncRead` and `AsyncWrite`, so sharing it via `Arc` stands in for splitting it.
#[derive(Debug)]
struct SharedStream(Arc<LocalSocketStream>);

impl AsyncRead for SharedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut &*self.0).poll_read(cx, buf)
    }
}

impl Connection {
    /// Opens a raw connection to `endpoint`. Most callers want [`crate::Client`] instead.
    pub async fn connect(endpoint: &crate::Endpoint) -> io::Result<Self> {
        endpoint.connect().await
    }

    pub(crate) fn new(stream: LocalSocketStream) -> Self {
        let stream = Arc::new(stream);
        Self {
            reader: BufReader::new(SharedStream(Arc::clone(&stream))),
            stream,
        }
    }

    /// Sends one line; `line` must end with `\n` and contain no other newline.
    pub async fn send_line(&mut self, line: &str) -> io::Result<()> {
        let mut writer = &*self.stream;
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await
    }

    /// Receives one line, including its `\n`, or `None` if the other side closed cleanly.
    pub async fn recv_line(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        let limit = u64::try_from(MAX_LINE_BYTES).unwrap_or(u64::MAX);
        let read = (&mut self.reader).take(limit).read_line(&mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        if !line.ends_with('\n') {
            let kind = if line.len() >= MAX_LINE_BYTES {
                io::ErrorKind::InvalidData
            } else {
                io::ErrorKind::UnexpectedEof
            };
            return Err(io::Error::new(kind, "incomplete or oversized line"));
        }
        Ok(Some(line))
    }
}
