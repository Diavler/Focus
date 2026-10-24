//! Calls to the daemon, one connection per call, with errors as text the window can show.
//!
//! A fresh connection each time means a restarted daemon is picked up without reconnect logic;
//! local connections are cheap enough for a once-a-second poll.

use std::io;

use focus_core::{BlockConfig, EditableSettings, ParkedItem};
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

/// The settings as written, for the settings editor.
pub async fn settings(endpoint: Endpoint) -> Result<EditableSettings, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    match client.request(&Request::Settings).await.map_err(text)? {
        Response::Settings(settings) => Ok(*settings),
        other => Err(format!("unexpected answer: {other:?}")),
    }
}

/// Everything parked.
pub async fn parking_lot(endpoint: Endpoint) -> Result<Vec<ParkedItem>, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    match client.request(&Request::ParkingLot).await.map_err(text)? {
        Response::ParkingLot { items } => Ok(items),
        other => Err(format!("unexpected answer: {other:?}")),
    }
}

/// The apps with a window open.
pub async fn apps(endpoint: Endpoint) -> Result<Vec<String>, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    match client.request(&Request::WindowedApps).await.map_err(text)? {
        Response::Apps { apps } => Ok(apps),
        other => Err(format!("unexpected answer: {other:?}")),
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
