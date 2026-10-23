//! Chrome's native messaging framing: a 4-byte length in native byte order, then that many bytes
//! of UTF-8 JSON.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest message accepted from the browser. Ours are small; this stops a bad length from
/// making the host allocate gigabytes.
pub const MAX_INCOMING: usize = 1024 * 1024;

/// Largest message Chrome accepts from a host.
pub const MAX_OUTGOING: usize = 1024 * 1024;

/// Reads one message, or `None` if the browser closed the pipe cleanly between messages.
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = usize::try_from(u32::from_ne_bytes(length)).unwrap_or(usize::MAX);
    if length > MAX_INCOMING {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {length} bytes is over the {MAX_INCOMING}-byte limit"),
        ));
    }
    let mut message = vec![0_u8; length];
    reader.read_exact(&mut message).await?;
    Ok(Some(message))
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

    #[tokio::test]
    async fn round_trips_messages_back_to_back() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, b"{\"a\":1}").await.unwrap();
        write_message(&mut buffer, b"[]").await.unwrap();
        let mut reader = buffer.as_slice();
        assert_eq!(
            read_message(&mut reader).await.unwrap().unwrap(),
            b"{\"a\":1}"
        );
        assert_eq!(read_message(&mut reader).await.unwrap().unwrap(), b"[]");
        assert_eq!(read_message(&mut reader).await.unwrap(), None, "clean end");
    }

    #[tokio::test]
    async fn rejects_oversized_and_truncated_messages() {
        let huge = u32::try_from(MAX_INCOMING + 1).unwrap().to_ne_bytes();
        assert!(read_message(&mut huge.as_slice()).await.is_err());

        let mut truncated = 10_u32.to_ne_bytes().to_vec();
        truncated.extend_from_slice(b"abc");
        assert!(read_message(&mut truncated.as_slice()).await.is_err());

        let too_big = vec![b' '; MAX_OUTGOING + 1];
        assert!(write_message(&mut Vec::new(), &too_big).await.is_err());
    }
}
