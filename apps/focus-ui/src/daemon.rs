//! Calls to the daemon, one connection per call, with errors as text the window can show.
//!
//! A fresh connection each time means a restarted daemon is picked up without reconnect logic;
//! local connections are cheap enough for a once-a-second poll.

use std::io;

use focus_core::{BlockConfig, Verdict};
use ipc_proto::{PlanView, Request, Response, StatusReport};
use ipc_transport::{Client, ClientError, Endpoint};

/// Why the status couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusError {
    /// What went wrong, for the window.
    pub message: String,
    /// Nothing is listening: the daemon isn't running, so starting it may help. False for a
    /// daemon that's slow, busy, owned by another user or speaking another protocol version,
    /// where starting another would do nothing.
    pub not_running: bool,
}

/// The daemon's status.
pub async fn status(endpoint: Endpoint) -> Result<StatusReport, StatusError> {
    let mut client = Client::connect(&endpoint)
        .await
        .map_err(|error| StatusError {
            not_running: is_not_running(&error),
            message: error.to_string(),
        })?;
    client.status().await.map_err(|error| StatusError {
        message: error.to_string(),
        not_running: false,
    })
}

/// Whether a connect error means nothing is listening.
fn is_not_running(error: &ClientError) -> bool {
    matches!(error, ClientError::Io(io) if matches!(io.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused))
}

/// The configurations in effect.
pub async fn configs(endpoint: Endpoint) -> Result<Vec<BlockConfig>, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    client.list_configs().await.map_err(text)
}

/// The calendar, tasks and automatic-start rules.
pub async fn plan(endpoint: Endpoint) -> Result<PlanView, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    match client.request(&Request::Plan).await.map_err(text)? {
        Response::Plan(plan) => Ok(*plan),
        other => Err(format!("unexpected answer: {other:?}")),
    }
}

/// Sends a request whose answer only matters if it's an error.
pub async fn act(endpoint: Endpoint, request: Request) -> Result<(), String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    client.request(&request).await.map(|_| ()).map_err(text)
}

/// What happens to `url` under the running session, or under `config`; worded for the window.
pub async fn check(
    endpoint: Endpoint,
    url: String,
    config: Option<String>,
) -> Result<String, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    let (verdict, decided_by) = client
        .rules_for(&url, config.as_deref())
        .await
        .map_err(text)?;
    Ok(match (verdict, decided_by) {
        (verdict, Some(config)) => format!("{config}: {verdict}"),
        (Verdict::Allow, None) => "No session is running, so everything is allowed.".to_owned(),
        (verdict, None) => verdict.to_string(),
    })
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
