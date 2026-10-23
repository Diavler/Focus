//! Calls to the daemon, one connection per call, with errors as text the window can show.
//!
//! A fresh connection each time means a restarted daemon is picked up without reconnect logic;
//! local connections are cheap enough for a once-a-second poll.

use focus_core::{BlockConfig, Verdict};
use ipc_proto::{Request, StatusReport};
use ipc_transport::{Client, Endpoint};

/// The daemon's status.
pub async fn status(endpoint: Endpoint) -> Result<StatusReport, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    client.status().await.map_err(text)
}

/// The configurations in effect.
pub async fn configs(endpoint: Endpoint) -> Result<Vec<BlockConfig>, String> {
    let mut client = Client::connect(&endpoint).await.map_err(text)?;
    client.list_configs().await.map_err(text)
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
