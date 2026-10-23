//! Encoding messages as versioned JSON lines.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::messages::DaemonError;

/// The protocol version this build speaks. Bump it on any incompatible message change.
pub const PROTOCOL_VERSION: u32 = 1;

/// Why a line couldn't be decoded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// The line is a message from a different protocol version.
    #[error("unsupported protocol version {version} (this build speaks {PROTOCOL_VERSION})")]
    UnsupportedVersion {
        /// The version in the line.
        version: u32,
    },
    /// The line isn't a valid message.
    #[error("malformed message: {message}")]
    Malformed {
        /// The JSON parser's explanation.
        message: String,
    },
}

impl From<DecodeError> for DaemonError {
    /// How a request that couldn't be decoded is answered.
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::UnsupportedVersion { version } => Self::UnsupportedVersion {
                daemon: PROTOCOL_VERSION,
                client: version,
            },
            DecodeError::Malformed { message } => Self::BadRequest { message },
        }
    }
}

#[derive(Serialize)]
struct EnvelopeOut<'a, T> {
    version: u32,
    message: &'a T,
}

/// The message part of a line; the version was already checked via [`VersionOnly`].
#[derive(Deserialize)]
struct EnvelopeIn<T> {
    message: T,
}

/// Just the version, read first so a newer message is reported as such rather than as malformed.
#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

/// One message as a single JSON line ending in `\n`. JSON escapes newlines inside strings, so the
/// only newline is the terminator.
pub fn encode<T: Serialize>(message: &T) -> String {
    let envelope = EnvelopeOut {
        version: PROTOCOL_VERSION,
        message,
    };
    // Every message type is plain data with string map keys, which serde_json always encodes.
    let mut line = serde_json::to_string(&envelope)
        .unwrap_or_else(|error| unreachable!("IPC messages always encode as JSON: {error}"));
    line.push('\n');
    line
}

/// Decodes one line (with or without its trailing newline).
pub fn decode<T: DeserializeOwned>(line: &str) -> Result<T, DecodeError> {
    let malformed = |error: serde_json::Error| DecodeError::Malformed {
        message: error.to_string(),
    };
    let VersionOnly { version } = serde_json::from_str(line).map_err(malformed)?;
    if version != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion { version });
    }
    let envelope: EnvelopeIn<T> = serde_json::from_str(line).map_err(malformed)?;
    Ok(envelope.message)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use focus_core::{
        BlockConfig, EndEarlyResponse, Mode, RuleAction, SessionError, SessionSpec, SessionState,
        Strictness, Timestamp, Verdict,
    };

    use super::*;
    use crate::{BrowserStatus, DaemonError, PendingSummary, Request, Response, StatusReport};

    fn round_trip<T>(message: T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let line = encode(&message);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "one line: {line:?}");
        assert_eq!(decode::<T>(&line), Ok(message));
    }

    #[test]
    fn every_request_round_trips() {
        for request in [
            Request::Status,
            Request::ListConfigs,
            Request::StartSession {
                config: "coding".into(),
                strictness: Strictness::Deep,
                duration_secs: 1500,
            },
            Request::EndSession,
            Request::ConfirmEndSession {
                reason: "line one\nline two".into(),
            },
            Request::CancelEndSession,
            Request::Heartbeat {
                browser: "chrome".into(),
                incognito_allowed: false,
            },
            Request::RulesFor {
                url: "https://reddit.com/".into(),
                config: Some("coding".into()),
            },
        ] {
            round_trip(request);
        }
    }

    #[test]
    fn every_response_round_trips() {
        let mut session = SessionState::default();
        let spec = SessionSpec {
            config: BlockConfig::new("coding", Mode::Blocklist),
            strictness: Strictness::Locked,
            duration: Duration::from_secs(60),
        };
        session.start(spec, Timestamp::from_millis(5)).unwrap();
        for response in [
            Response::Status(StatusReport {
                now: Timestamp::from_millis(10),
                session,
                pending_changes: vec![PendingSummary {
                    config: "coding".into(),
                    apply_after: Timestamp::from_millis(99),
                    deleted: false,
                }],
                settings_problem: Some("bad pattern".into()),
                browsers: vec![BrowserStatus {
                    browser: "edge".into(),
                    last_seen: Timestamp::from_millis(9),
                    incognito_allowed: true,
                }],
            }),
            Response::Configs {
                configs: vec![BlockConfig::new("writing", Mode::Allowlist)],
            },
            Response::EndSession {
                outcome: EndEarlyResponse::WaitUntil(Timestamp::from_millis(30)),
            },
            Response::Verdict {
                verdict: Verdict::Restrict(RuleAction::HideFeed),
                config: None,
            },
            Response::Done,
            Response::Error {
                error: DaemonError::Session {
                    error: SessionError::StillWaiting {
                        remaining: Duration::from_secs(3),
                    },
                },
            },
            Response::Error {
                error: DaemonError::UnknownConfig {
                    name: "gaming".into(),
                },
            },
        ] {
            round_trip(response);
        }
    }

    #[test]
    fn wire_format_is_stable() {
        assert_eq!(
            encode(&Request::RulesFor {
                url: "x.com".into(),
                config: None
            }),
            "{\"version\":1,\"message\":{\"type\":\"rules_for\",\"url\":\"x.com\",\"config\":null}}\n"
        );
        assert_eq!(
            decode::<Request>(r#"{"version":1,"message":{"type":"status"}}"#),
            Ok(Request::Status)
        );
    }

    #[test]
    fn other_versions_are_reported_as_such() {
        assert_eq!(
            decode::<Request>(r#"{"version":2,"message":{"type":"teleport"}}"#),
            Err(DecodeError::UnsupportedVersion { version: 2 })
        );
    }

    #[test]
    fn junk_is_malformed() {
        for line in [
            "",
            "not json",
            "{}",
            r#"{"version":1,"message":{"type":"teleport"}}"#,
        ] {
            assert!(
                matches!(decode::<Request>(line), Err(DecodeError::Malformed { .. })),
                "{line:?}"
            );
        }
    }
}
