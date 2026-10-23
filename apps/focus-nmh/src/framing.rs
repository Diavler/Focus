//! Chrome's native messaging framing: a 4-byte length in native byte order, then that many bytes
//! of UTF-8 JSON.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest message accepted from the browser. Ours are small; this stops a bad length from
/// making the host allocate gigabytes.
pub const MAX_INCOMING: usize = 1024 * 1024;

/// Largest message Chrome accepts from a host.
pub const MAX_OUTGOING: usize = 1024 * 1024;

/// One framed message from the browser.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// A message within the size limit.
    Message(Vec<u8>),
    /// A message over [`MAX_INCOMING`], already skipped so the next one can be read.
    TooLarge(usize),
}

/// Reads one message, or `None` if the browser closed the pipe cleanly between messages. A pipe
/// that closes partway through a message is an error.
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Frame>> {
    let mut length = [0_u8; 4];
    // Only a close before the first byte is clean; read_exact reports one after it as an error.
    if reader.read(&mut length[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..]).await?;
    let length = u32::from_ne_bytes(length);
    let size = usize::try_from(length).unwrap_or(usize::MAX);
    if size > MAX_INCOMING {
        let skipped =
            tokio::io::copy(&mut reader.take(u64::from(length)), &mut tokio::io::sink()).await?;
        if skipped < u64::from(length) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        return Ok(Some(Frame::TooLarge(size)));
    }
    let mut message = vec![0_u8; size];
    reader.read_exact(&mut message).await?;
    Ok(Some(Frame::Message(message)))
}

/// Writes one message and flushes it, so the browser sees it straight away.
pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &[u8],
) -> io::Result<()> {
    let length = u32::try_from(message.len())
        .ok()
        .filter(|_| message.len() <= MAX_OUTGOING)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "message is over Chrome's 1 MiB limit",
            )
        })?;
    writer.write_all(&length.to_ne_bytes()).await?;
    writer.write_all(message).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: &[u8]) -> Vec<u8> {
        let mut framed = u32::try_from(bytes.len()).unwrap().to_ne_bytes().to_vec();
        framed.extend_from_slice(bytes);
        framed
    }

    #[tokio::test]
    async fn round_trips_messages_back_to_back() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, b"{\"a\":1}").await.unwrap();
        write_message(&mut buffer, b"[]").await.unwrap();
        let mut reader = buffer.as_slice();
        let next = read_message(&mut reader).await.unwrap();
        assert_eq!(next, Some(Frame::Message(b"{\"a\":1}".to_vec())));
        let next = read_message(&mut reader).await.unwrap();
        assert_eq!(next, Some(Frame::Message(b"[]".to_vec())));
        assert_eq!(read_message(&mut reader).await.unwrap(), None, "clean end");
    }

    #[tokio::test]
    async fn skips_an_oversized_message_and_reads_the_next() {
        let mut stream = frame(&vec![b'x'; MAX_INCOMING + 1]);
        stream.extend(frame(b"{}"));
        let mut reader = stream.as_slice();
        let next = read_message(&mut reader).await.unwrap();
        assert_eq!(next, Some(Frame::TooLarge(MAX_INCOMING + 1)));
        let next = read_message(&mut reader).await.unwrap();
        assert_eq!(next, Some(Frame::Message(b"{}".to_vec())));
    }

    #[tokio::test]
    async fn a_pipe_closed_mid_message_is_an_error() {
        let half_header = [7_u8, 0];
        assert!(read_message(&mut half_header.as_slice()).await.is_err());

        let mut truncated = 10_u32.to_ne_bytes().to_vec();
        truncated.extend_from_slice(b"abc");
        assert!(read_message(&mut truncated.as_slice()).await.is_err());

        let too_big = vec![b' '; MAX_OUTGOING + 1];
        assert!(write_message(&mut Vec::new(), &too_big).await.is_err());
    }
}
