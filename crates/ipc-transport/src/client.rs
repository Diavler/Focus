//! A typed client for the daemon.

use std::io;
use std::time::Duration;

use focus_core::{BlockConfig, EndEarlyResponse, Strictness, Verdict};
use ipc_proto::{DaemonError, Request, Response, StatusReport, decode, encode};
use thiserror::Error;

use crate::connection::Connection;
use crate::endpoint::Endpoint;

/// Why a request didn't get a successful answer.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Nothing is listening, or the connection broke.
    #[error("can't reach the Focus daemon: {0}")]
    Io(#[from] io::Error),
    /// The daemon closed the connection without answering.
    #[error("the Focus daemon closed the connection")]
    Closed,
    /// The answer wasn't what this request expects.
    #[error("unexpected answer from the Focus daemon: {0}")]
    Protocol(String),
    /// The daemon refused the request.
    #[error(transparent)]
    Daemon(#[from] DaemonError),
}

/// A connection to the daemon that sends requests one at a time.
#[derive(Debug)]
pub struct Client {
    connection: Connection,
}

impl Client {
    /// Connects to the daemon at `endpoint`.
    pub async fn connect(endpoint: &Endpoint) -> Result<Self, ClientError> {
        Ok(Self {
            connection: endpoint.connect().await?,
        })
    }

    /// Sends `request` and returns the answer; a [`Response::Error`] becomes [`ClientError::Daemon`].
    pub async fn request(&mut self, request: &Request) -> Result<Response, ClientError> {
        self.connection.send_line(&encode(request)).await?;
        let line = self
            .connection
            .recv_line()
            .await?
            .ok_or(ClientError::Closed)?;
        match decode::<Response>(&line).map_err(|error| ClientError::Protocol(error.to_string()))? {
            Response::Error { error } => Err(ClientError::Daemon(error)),
            response => Ok(response),
        }
    }

    /// The daemon's status.
    pub async fn status(&mut self) -> Result<StatusReport, ClientError> {
        match self.request(&Request::Status).await? {
            Response::Status(status) => Ok(status),
            other => Err(unexpected(&other)),
        }
    }

    /// The configurations in effect.
    pub async fn list_configs(&mut self) -> Result<Vec<BlockConfig>, ClientError> {
        match self.request(&Request::ListConfigs).await? {
            Response::Configs { configs } => Ok(configs),
            other => Err(unexpected(&other)),
        }
    }

    /// Starts a session.
    pub async fn start_session(
        &mut self,
        config: &str,
        strictness: Strictness,
        duration: Duration,
    ) -> Result<(), ClientError> {
        let request = Request::StartSession {
            config: config.to_owned(),
            strictness,
            duration_secs: duration.as_secs(),
        };
        self.expect_done(&request).await
    }

    /// Asks to end the running session early.
    pub async fn end_session(&mut self) -> Result<EndEarlyResponse, ClientError> {
        match self.request(&Request::EndSession).await? {
            Response::EndSession { outcome } => Ok(outcome),
            other => Err(unexpected(&other)),
        }
    }

    /// Ends a Deep session after its wait, with a typed reason.
    pub async fn confirm_end_session(&mut self, reason: &str) -> Result<(), ClientError> {
        let request = Request::ConfirmEndSession {
            reason: reason.to_owned(),
        };
        self.expect_done(&request).await
    }

    /// Withdraws a pending end request.
    pub async fn cancel_end_session(&mut self) -> Result<(), ClientError> {
        self.expect_done(&Request::CancelEndSession).await
    }

    /// Reports that a browser's extension is alive.
    pub async fn heartbeat(
        &mut self,
        browser: &str,
        incognito_allowed: bool,
    ) -> Result<(), ClientError> {
        let request = Request::Heartbeat {
            browser: browser.to_owned(),
            incognito_allowed,
        };
        self.expect_done(&request).await
    }

    /// The verdict for `url` under the running session, or under `config` if given.
    pub async fn rules_for(
        &mut self,
        url: &str,
        config: Option<&str>,
    ) -> Result<(Verdict, Option<String>), ClientError> {
        let request = Request::RulesFor {
            url: url.to_owned(),
            config: config.map(str::to_owned),
        };
        match self.request(&request).await? {
            Response::Verdict { verdict, config } => Ok((verdict, config)),
            other => Err(unexpected(&other)),
        }
    }

    async fn expect_done(&mut self, request: &Request) -> Result<(), ClientError> {
        match self.request(request).await? {
            Response::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }
}

fn unexpected(response: &Response) -> ClientError {
    ClientError::Protocol(format!("{response:?}"))
}
