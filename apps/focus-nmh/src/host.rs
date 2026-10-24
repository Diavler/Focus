//! Relays the extension's requests to the daemon and the answers back.
//!
//! The extension sends `{"id":7,"version":1,"message":{...}}`, the same envelope as the daemon
//! protocol plus an `id`, and gets `{"id":7,"version":1,"message":<Response>}` back. Only requests
//! the extension needs are relayed, so a compromised page script can't start or end sessions.

use std::io;

use ipc_proto::{DaemonError, PROTOCOL_VERSION, Request, Response, decode};
use ipc_transport::{Client, ClientError, Endpoint};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::framing::{Frame, MAX_INCOMING, MAX_OUTGOING, read_message, write_message};

/// Just the `id`, read on its own so even a malformed request gets an answer it can be matched to.
#[derive(Deserialize)]
struct IdOnly {
    id: u64,
}

/// One answer to the extension.
#[derive(Serialize)]
struct Outgoing<'a> {
    /// The request's `id`, or 0 if it couldn't be read.
    id: u64,
    version: u32,
    message: &'a Response,
}

/// Relays messages until the browser closes the pipe. A bad or oversized message gets an error
/// answer; only a broken pipe ends the relay.
///
/// When the daemon can't be reached, `no_session_saved` is asked whether the saved state shows no
/// running session. If so, Focus was stopped (the kill switch saves that before the daemon
/// exits), and the answer is [`DaemonError::Stopped`] so the extension lifts its blocks. Otherwise
/// it's [`DaemonError::Unreachable`] and the extension keeps the last session's blocks, so killing
/// the daemon isn't a way out of a session.
pub async fn run<R, W>(
    reader: &mut R,
    writer: &mut W,
    endpoint: &Endpoint,
    no_session_saved: impl Fn() -> bool,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut daemon: Option<Client> = None;
    while let Some(frame) = read_message(reader).await? {
        let (id, response) = match frame {
            Frame::TooLarge(size) => (
                0,
                error_response(DaemonError::BadRequest {
                    message: format!(
                        "message of {size} bytes is over the {MAX_INCOMING}-byte limit"
                    ),
                }),
            ),
            Frame::Message(bytes) => {
                let id = serde_json::from_slice::<IdOnly>(&bytes).map_or(0, |only| only.id);
                let response = match answer(&bytes, &mut daemon, endpoint).await {
                    Response::Error {
                        error: DaemonError::Unreachable { message },
                    } if no_session_saved() => error_response(DaemonError::Stopped { message }),
                    response => response,
                };
                (id, response)
            }
        };
        let mut bytes = encode(id, &response);
        if bytes.len() > MAX_OUTGOING {
            bytes = encode(
                id,
                &error_response(DaemonError::Internal {
                    message: "the answer is too large to send to the browser".into(),
                }),
            );
        }
        write_message(writer, &bytes).await?;
    }
    Ok(())
}

/// Checks one request and forwards it.
async fn answer(bytes: &[u8], daemon: &mut Option<Client>, endpoint: &Endpoint) -> Response {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return error_response(DaemonError::BadRequest {
            message: "the message isn't UTF-8".into(),
        });
    };
    // `decode` reads `version` and `message` and ignores the extra `id`.
    let request = match decode::<Request>(text) {
        Ok(request) => request,
        Err(error) => return error_response(error.into()),
    };
    if !allowed_from_browser(&request) {
        return error_response(DaemonError::BadRequest {
            message: "the browser extension may only send Heartbeat, RulesFor, Status, Unlock and DelayVisit".into(),
        });
    }
    forward(&request, daemon, endpoint).await
}

/// The requests the extension needs; everything else stays with the UI.
fn allowed_from_browser(request: &Request) -> bool {
    matches!(
        request,
        Request::Heartbeat { .. }
            | Request::RulesFor { .. }
            | Request::Status
            | Request::Unlock { .. }
            | Request::DelayVisit { .. }
    )
}

/// Sends `request` over the kept connection. If that connection turns out to be dead (the daemon
/// restarted since the last message), reconnects and sends it once more. A request that timed
/// out is not resent: the daemon may already have handled it, and waiting again would hold up
/// the heartbeats queued behind it.
async fn forward(request: &Request, daemon: &mut Option<Client>, endpoint: &Endpoint) -> Response {
    let mut retried = false;
    loop {
        let mut client = match daemon.take() {
            Some(client) => client,
            None => match Client::connect(endpoint).await {
                Ok(client) => client,
                Err(error) => return unreachable(&error),
            },
        };
        match client.request(request).await {
            Ok(response) => {
                *daemon = Some(client);
                return response;
            }
            Err(ClientError::Daemon(error)) => {
                *daemon = Some(client);
                return error_response(error);
            }
            // The connection may be out of step after a garbled answer, so it isn't kept.
            Err(ClientError::Protocol(message)) => {
                return error_response(DaemonError::Internal { message });
            }
            Err(error) => {
                let timed_out =
                    matches!(&error, ClientError::Io(io) if io.kind() == io::ErrorKind::TimedOut);
                if timed_out || retried {
                    return unreachable(&error);
                }
                retried = true;
            }
        }
    }
}

fn encode(id: u64, response: &Response) -> Vec<u8> {
    let outgoing = Outgoing {
        id,
        version: PROTOCOL_VERSION,
        message: response,
    };
    // Responses are plain data with string keys, which serde_json always encodes.
    serde_json::to_vec(&outgoing).unwrap_or_default()
}

fn unreachable(error: &ClientError) -> Response {
    error_response(DaemonError::Unreachable {
        message: error.to_string(),
    })
}

fn error_response(error: DaemonError) -> Response {
    Response::Error { error }
}
