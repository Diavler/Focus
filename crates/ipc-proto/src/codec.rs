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

    use focus_core::ports::{SecretKey, Silencing};
    use focus_core::{
        AutoStartRule, BlockConfig, CalendarEvent, ConfigEntry, EditableSettings, EndEarlyResponse,
        Friction, Hotkey, Mode, ParkError, ParkingLot, PlanSource, PlannedStart, RuleAction,
        RulePatterns, SessionError, SessionSpec, SessionState, StopError, Strictness, Task, Theme,
        Timestamp, UnlockError, Verdict,
    };

    use super::*;
    use crate::{
        BrowserStatus, DaemonError, ParkingStatus, PendingSummary, PlanStatus, PlanView, Request,
        Response, SourceState, SourceStatus, StatusReport,
    };

    fn round_trip<T>(message: T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let line = encode(&message);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "one line: {line:?}");
        assert_eq!(decode::<T>(&line), Ok(message));
    }

    fn settings() -> EditableSettings {
        EditableSettings {
            start_at_login: true,
            lists: Vec::new(),
            configs: vec![ConfigEntry {
                name: "coding".into(),
                mode: Mode::Blocklist,
                include: Vec::new(),
                patterns: RulePatterns {
                    block: vec!["x.com".into()],
                    ..RulePatterns::default()
                },
                apps: vec!["discord".into()],
            }],
            auto_start: vec![AutoStartRule {
                config: "coding".into(),
                strictness: Strictness::Deep,
                calendar_keywords: vec!["#focus".into()],
                task_labels: Vec::new(),
                task_duration: AutoStartRule::DEFAULT_TASK_DURATION,
            }],
            friction: Friction {
                tab_cap: 6,
                ..Friction::default()
            },
            parking_hotkey: Hotkey::Off,
            theme: Theme::Dark,
            timer_on_top: true,
            revision: "abc".into(),
        }
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
            Request::SetSecret {
                key: SecretKey::TodoistToken,
                value: Some("token".into()),
            },
            Request::SetSecret {
                key: SecretKey::CalendarUrl,
                value: None,
            },
            Request::Plan,
            Request::Settings,
            Request::SaveSettings {
                settings: settings(),
            },
            Request::WindowedApps,
            Request::RequestStop,
            Request::ConfirmStop {
                reason: "emergency".into(),
            },
            Request::CancelStop,
            Request::Unlock {
                url: "https://x.com/".into(),
                reason: "the thread has the fix".into(),
            },
            Request::DelayVisit {
                url: "https://news.ycombinator.com/".into(),
            },
            Request::Park {
                text: "Serde flatten".into(),
                url: Some("https://serde.rs/attr-flatten.html".into()),
            },
            Request::ParkingLot,
            Request::Unpark { id: 3 },
            Request::ClearSentParked,
            Request::OpenNotificationSettings,
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
            Response::Status(Box::new(StatusReport {
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
                foreground_app: Some("code".into()),
                start_at_login: true,
                start_at_login_off_after: Some(Timestamp::from_millis(77)),
                start_at_login_problem: None,
                stop_allowed_at: Some(Timestamp::from_millis(70_000)),
                stop_reason_required: true,
                friction: Friction::default(),
                friction_loosen_after: Some(Timestamp::from_millis(88)),
                unlocks_left: 2,
                parking_lot: ParkingStatus {
                    total: 3,
                    waiting: 1,
                    refused: 1,
                    inbox: SourceState::NotConfigured,
                    revision: 7,
                },
                notifications: Some(Silencing::AskUser),
                parking_hotkey: Hotkey::CtrlAltP,
                theme: Theme::Light,
                timer_on_top: true,
                plan: PlanStatus {
                    calendar: SourceStatus {
                        state: SourceState::Failing {
                            problem: "offline".into(),
                            offline: true,
                        },
                        fetched_at: Some(Timestamp::from_millis(3)),
                        items: 4,
                    },
                    tasks: SourceStatus::default(),
                },
            })),
            Response::Plan(Box::new(PlanView {
                now: Timestamp::from_millis(10),
                events: vec![CalendarEvent {
                    id: "a@1".into(),
                    title: "Deep work".into(),
                    start: Timestamp::from_millis(20),
                    end: Timestamp::from_millis(40),
                    all_day: false,
                }],
                tasks: vec![Task {
                    id: "1".into(),
                    title: "Write".into(),
                    labels: vec!["deep".into()],
                    due_date: Some("2026-10-24".into()),
                    due_at: None,
                    duration: Some(Duration::from_secs(60)),
                    priority: focus_core::TaskPriority::default(),
                    project: None,
                    section: None,
                }],
                upcoming: vec![PlannedStart {
                    key: "calendar:a@1".into(),
                    source: PlanSource::Calendar,
                    title: "Deep work".into(),
                    config: "coding".into(),
                    strictness: Strictness::Deep,
                    starts_at: Timestamp::from_millis(20),
                    ends_at: Timestamp::from_millis(40),
                }],
            })),
            Response::Configs {
                configs: vec![BlockConfig::new("writing", Mode::Allowlist)],
            },
            Response::Settings(Box::new(settings())),
            Response::Apps {
                apps: vec!["discord".into()],
            },
            Response::Stop {
                allowed_at: Timestamp::from_millis(60_000),
                reason_required: true,
            },
            Response::Error {
                error: DaemonError::Stop {
                    error: StopError::StillWaiting {
                        remaining: Duration::from_secs(5),
                    },
                },
            },
            Response::Error {
                error: DaemonError::SettingsChanged,
            },
            Response::Unlocked {
                until: Timestamp::from_millis(600_000),
            },
            Response::Delay { seconds: 20 },
            Response::ParkingLot {
                items: {
                    let mut lot = ParkingLot::default();
                    lot.park("a thought", None, Timestamp::from_millis(1))
                        .unwrap();
                    lot.items().to_vec()
                },
            },
            Response::Error {
                error: DaemonError::Park {
                    error: ParkError::Full { max: 500 },
                },
            },
            Response::Error {
                error: DaemonError::Unlock {
                    error: UnlockError::BudgetSpent {
                        next_at: Some(Timestamp::from_millis(5)),
                    },
                },
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
