//! Relays the extension's requests to the daemon and the answers back.
//!
//! The extension sends `{"id":7,"version":1,"message":{...}}`, the same envelope as the daemon
//! protocol plus an `id`, and gets `{"id":7,"version":1,"message":<Response>}` back. Only requests
//! the extension needs are relayed, so a compromised page script can't start or end sessions.

use std::io;

use ipc_proto::{DaemonError, PROTOCOL_VERSION, Request, Response};
use ipc_transport::{Client, ClientError, Endpoint};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::framing::{read_message, write_message};

/// One request from the extension.
#[derive(Deserialize)]
struct Incoming {
    id: u64,
    version: u32,
    message: serde_json::Value,
}

/// One answer to the extension.
#[derive(Serialize)]
struct Outgoing<'a> {
    /// The request's `id`, or 0 if it couldn't be read.
    id: u64,
    version: u32,
    message: &'a Response,
}

/// Relays messages until the browser closes the pipe.
pub async fn run<R, W>(reader: &mut R, writer: &mut W, endpoint: &Endpoint) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut daemon: Option<Client> = None;
    while let Some(message) = read_message(reader).await? {
        let (id, response) = match serde_json::from_slice::<Incoming>(&message) {
            Ok(incoming) => (incoming.id, answer(incoming, &mut daemon, endpoint).await),
            Err(error) => (
                0,
                error_response(DaemonError::BadRequest {
                    message: error.to_string(),
                }),
            ),
        };
        let outgoing = Outgoing {
            id,
            version: PROTOCOL_VERSION,
            message: &response,
        };
        let bytes = serde_json::to_vec(&outgoing)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_message(writer, &bytes).await?;
    }
    Ok(())
}

/// Checks one request and forwards it.
async fn answer(incoming: Incoming, daemon: &mut Option<Client>, endpoint: &Endpoint) -> Response {
    if incoming.version != PROTOCOL_VERSION {
        return error_response(DaemonError::UnsupportedVersion {
            daemon: PROTOCOL_VERSION,
            client: incoming.version,
        });
    }
    let request = match serde_json::from_value::<Request>(incoming.message) {
        Ok(request) => request,
        Err(error) => {
            return error_response(DaemonError::BadRequest {
                message: error.to_string(),
            });
        }
    };
    if !allowed_from_browser(&request) {
        return error_response(DaemonError::BadRequest {
            message: "the browser extension may only send Heartbeat, RulesFor and Status".into(),
        });
    }
    forward(&request, daemon, endpoint).await
}

/// The requests the extension needs; everything else stays with the UI.
fn allowed_from_browser(request: &Request) -> bool {
    matches!(
        request,
        Request::Heartbeat { .. } | Request::RulesFor { .. } | Request::Status
    )
}

/// Sends `request` over the kept connection, reconnecting once if it broke (e.g. the daemon
/// restarted since the last message).
async fn forward(request: &Request, daemon: &mut Option<Client>, endpoint: &Endpoint) -> Response {
    for attempt in 0..2 {
        if daemon.is_none() {
            match Client::connect(endpoint).await {
                Ok(client) => *daemon = Some(client),
                Err(error) => return unreachable(&error),
            }
        }
        let Some(client) = daemon.as_mut() else {
            continue;
        };
        match client.request(request).await {
            Ok(response) => return response,
            Err(ClientError::Daemon(error)) => return error_response(error),
            Err(ClientError::Protocol(message)) => {
                return error_response(DaemonError::Internal { message });
            }
            Err(error) => {
                *daemon = None;
                if attempt == 1 {
                    return unreachable(&error);
                }
            }
        }
    }
    unreachable(&"no connection")
}

fn unreachable(error: &dyn std::fmt::Display) -> Response {
    error_response(DaemonError::Unreachable {
        message: error.to_string(),
    })
}

fn error_response(error: DaemonError) -> Response {
    Response::Error { error }
}
