//! The session state machine: Idle → Running → Ended, with end-early rules per strictness.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::BlockConfig;
use crate::strictness::{EndEarlyPolicy, OverridePolicy, Strictness};
use crate::time::Timestamp;

/// What to run: a configuration, a strictness level and a duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpec {
    /// The rules, copied in at start so later edits don't loosen a running session.
    pub config: BlockConfig,
    /// How hard the session is to end early or bypass.
    pub strictness: Strictness,
    /// How long the session lasts.
    pub duration: Duration,
}

/// A session in progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningSession {
    spec: SessionSpec,
    started_at: Timestamp,
    ends_at: Timestamp,
    end_requested_at: Option<Timestamp>,
}

/// A finished session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndedSession {
    spec: SessionSpec,
    started_at: Timestamp,
    ended_at: Timestamp,
    reason: EndReason,
}

/// Why a session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The timer ran out.
    Completed,
    /// It was ended early; Deep sessions record the typed reason.
    EndedEarly {
        /// The reason given, if one was required.
        reason: Option<String>,
    },
}

/// Where the session lifecycle is. Starts `Idle`; a new session can start once the last one ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// No session has run yet.
    #[default]
    Idle,
    /// A session is in progress.
    Running(RunningSession),
    /// The last session has finished.
    Ended(EndedSession),
}

/// The result of asking to end a session early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndEarlyResponse {
    /// The session has ended.
    Ended,
    /// The session can be ended with a typed reason from this time on.
    WaitUntil(Timestamp),
}

/// Why a session transition was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionError {
    /// A session is already running.
    #[error("a session is already running")]
    AlreadyRunning,
    /// The requested duration was zero.
    #[error("a session needs a duration longer than zero")]
    ZeroDuration,
    /// No session is running.
    #[error("no session is running")]
    NotRunning,
    /// Locked sessions can't be ended early.
    #[error("a locked session can't be ended early")]
    Locked,
    /// Confirming without asking to end first.
    #[error("ask to end the session before confirming")]
    NoEndRequest,
    /// The wait after asking to end isn't over yet.
    #[error("wait {remaining:?} more before ending the session")]
    StillWaiting {
        /// Time left to wait.
        remaining: Duration,
    },
    /// Ending a Deep session needs a typed reason.
    #[error("type a reason to end the session early")]
    ReasonRequired,
}

impl SessionState {
    /// Starts a session at `now`. Fails if one is running (after expiring a finished one).
    pub fn start(&mut self, spec: SessionSpec, now: Timestamp) -> Result<(), SessionError> {
        self.tick(now);
        if self.running().is_some() {
            return Err(SessionError::AlreadyRunning);
        }
        if spec.duration.is_zero() {
            return Err(SessionError::ZeroDuration);
        }
        *self = Self::Running(RunningSession {
            ends_at: now.saturating_add(spec.duration),
            spec,
            started_at: now,
            end_requested_at: None,
        });
        Ok(())
    }

    /// Ends a running session whose timer has run out. Returns `true` if this call ended it.
    pub fn tick(&mut self, now: Timestamp) -> bool {
        match self.running() {
            Some(running) if now >= running.ends_at => {
                let ends_at = running.ends_at;
                self.finish(ends_at, EndReason::Completed);
                true
            }
            _ => false,
        }
    }

    /// The running session, if there is one.
    pub fn running(&self) -> Option<&RunningSession> {
        match self {
            Self::Running(running) => Some(running),
            _ => None,
        }
    }

    /// The last finished session, if the machine is in `Ended`.
    pub fn ended(&self) -> Option<&EndedSession> {
        match self {
            Self::Ended(ended) => Some(ended),
            _ => None,
        }
    }

    /// Asks to end early: Light ends now, Deep starts the wait, Locked refuses.
    ///
    /// Asking again during a Deep wait doesn't restart it.
    pub fn request_end_early(&mut self, now: Timestamp) -> Result<EndEarlyResponse, SessionError> {
        let running = self.running_after_tick(now)?;
        match running.spec.strictness.end_early_policy() {
            EndEarlyPolicy::Immediate => {
                self.finish(now, EndReason::EndedEarly { reason: None });
                Ok(EndEarlyResponse::Ended)
            }
            EndEarlyPolicy::WaitAndReason { wait } => {
                let requested_at = *running.end_requested_at.get_or_insert(now);
                Ok(EndEarlyResponse::WaitUntil(
                    requested_at.saturating_add(wait),
                ))
            }
            EndEarlyPolicy::Forbidden => Err(SessionError::Locked),
        }
    }

    /// Ends a Deep session once its wait is over, recording the typed `reason`.
    pub fn confirm_end_early(&mut self, reason: &str, now: Timestamp) -> Result<(), SessionError> {
        let running = self.running_after_tick(now)?;
        if running.spec.strictness == Strictness::Locked {
            return Err(SessionError::Locked);
        }
        let allowed_at = running.end_allowed_at().ok_or(SessionError::NoEndRequest)?;
        if now < allowed_at {
            return Err(SessionError::StillWaiting {
                remaining: allowed_at.saturating_duration_since(now),
            });
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(SessionError::ReasonRequired);
        }
        self.finish(
            now,
            EndReason::EndedEarly {
                reason: Some(reason.to_owned()),
            },
        );
        Ok(())
    }

    /// Withdraws a pending request to end early; a later request starts a fresh wait.
    pub fn cancel_end_request(&mut self) -> Result<(), SessionError> {
        match self {
            Self::Running(running) => {
                running.end_requested_at = None;
                Ok(())
            }
            _ => Err(SessionError::NotRunning),
        }
    }

    /// Expires a finished session, then returns the running one mutably.
    fn running_after_tick(&mut self, now: Timestamp) -> Result<&mut RunningSession, SessionError> {
        self.tick(now);
        match self {
            Self::Running(running) => Ok(running),
            _ => Err(SessionError::NotRunning),
        }
    }

    /// Moves a running session to `Ended`; does nothing otherwise.
    fn finish(&mut self, ended_at: Timestamp, reason: EndReason) {
        *self = match std::mem::take(self) {
            Self::Running(running) => Self::Ended(EndedSession {
                spec: running.spec,
                started_at: running.started_at,
                ended_at,
                reason,
            }),
            other => other,
        };
    }
}

impl RunningSession {
    /// What this session runs.
    pub fn spec(&self) -> &SessionSpec {
        &self.spec
    }

    /// When it started.
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// When the timer runs out.
    pub fn ends_at(&self) -> Timestamp {
        self.ends_at
    }

    /// Time until the timer runs out, or zero if it already has.
    pub fn time_left(&self, now: Timestamp) -> Duration {
        self.ends_at.saturating_duration_since(now)
    }

    /// When a pending end-early request was made, if there is one.
    pub fn end_requested_at(&self) -> Option<Timestamp> {
        self.end_requested_at
    }

    /// When a pending end-early request may be confirmed, if there is one.
    pub fn end_allowed_at(&self) -> Option<Timestamp> {
        let requested_at = self.end_requested_at?;
        match self.spec.strictness.end_early_policy() {
            EndEarlyPolicy::WaitAndReason { wait } => Some(requested_at.saturating_add(wait)),
            EndEarlyPolicy::Immediate => Some(requested_at),
            EndEarlyPolicy::Forbidden => None,
        }
    }

    /// Whether restricted pages can be opened "just this once" in this session.
    pub fn override_policy(&self) -> OverridePolicy {
        self.spec.strictness.override_policy()
    }
}

impl EndedSession {
    /// What this session ran.
    pub fn spec(&self) -> &SessionSpec {
        &self.spec
    }

    /// When it started.
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// When it ended.
    pub fn ended_at(&self) -> Timestamp {
        self.ended_at
    }

    /// Why it ended.
    pub fn reason(&self) -> &EndReason {
        &self.reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn spec(strictness: Strictness) -> SessionSpec {
        SessionSpec {
            config: BlockConfig::new("coding", Mode::Blocklist),
            strictness,
            duration: HOUR,
        }
    }

    fn started(strictness: Strictness, clock: &FakeClock) -> SessionState {
        let mut state = SessionState::default();
        state.start(spec(strictness), clock.now()).unwrap();
        state
    }

    #[test]
    fn starts_idle_and_runs_for_the_duration() {
        let clock = FakeClock::default();
        let mut state = SessionState::default();
        assert_eq!(state, SessionState::Idle);
        state.start(spec(Strictness::Light), clock.now()).unwrap();
        let running = state.running().unwrap();
        assert_eq!(running.started_at(), clock.now());
        assert_eq!(running.ends_at(), clock.now().saturating_add(HOUR));
        assert_eq!(running.time_left(clock.now()), HOUR);
    }

    #[test]
    fn refuses_zero_duration_and_a_second_session() {
        let clock = FakeClock::default();
        let mut state = SessionState::default();
        let zero = SessionSpec {
            duration: Duration::ZERO,
            ..spec(Strictness::Light)
        };
        assert_eq!(
            state.start(zero, clock.now()),
            Err(SessionError::ZeroDuration)
        );
        state.start(spec(Strictness::Light), clock.now()).unwrap();
        assert_eq!(
            state.start(spec(Strictness::Deep), clock.now()),
            Err(SessionError::AlreadyRunning)
        );
    }

    #[test]
    fn completes_when_the_timer_runs_out() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Locked, &clock);
        let ends_at = state.running().unwrap().ends_at();

        clock.advance(HOUR - Duration::from_millis(1));
        assert!(!state.tick(clock.now()));
        assert_eq!(
            state.running().unwrap().time_left(clock.now()),
            Duration::from_millis(1)
        );

        clock.advance(Duration::from_secs(5));
        assert!(state.tick(clock.now()));
        let ended = state.ended().unwrap();
        assert_eq!(ended.reason(), &EndReason::Completed);
        assert_eq!(
            ended.ended_at(),
            ends_at,
            "records the scheduled end, not the late tick"
        );
        assert!(!state.tick(clock.now()), "only the first tick ends it");
    }

    #[test]
    fn a_new_session_can_start_after_one_ends() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Light, &clock);
        clock.advance(HOUR * 2);
        state.start(spec(Strictness::Deep), clock.now()).unwrap();
        assert_eq!(state.running().unwrap().spec().strictness, Strictness::Deep);
    }

    #[test]
    fn light_ends_with_one_request() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Light, &clock);
        clock.advance(Duration::from_secs(60));
        assert_eq!(
            state.request_end_early(clock.now()),
            Ok(EndEarlyResponse::Ended)
        );
        let ended = state.ended().unwrap();
        assert_eq!(ended.reason(), &EndReason::EndedEarly { reason: None });
        assert_eq!(ended.ended_at(), clock.now());
    }

    #[test]
    fn deep_needs_the_wait_and_a_reason() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Deep, &clock);
        assert_eq!(
            state.confirm_end_early("done", clock.now()),
            Err(SessionError::NoEndRequest)
        );

        let allowed_at = clock.now().saturating_add(Strictness::DEEP_END_WAIT);
        assert_eq!(
            state.request_end_early(clock.now()),
            Ok(EndEarlyResponse::WaitUntil(allowed_at))
        );
        clock.advance(Duration::from_secs(10));
        assert_eq!(
            state.request_end_early(clock.now()),
            Ok(EndEarlyResponse::WaitUntil(allowed_at)),
            "asking again doesn't restart the wait"
        );
        assert_eq!(
            state.confirm_end_early("done", clock.now()),
            Err(SessionError::StillWaiting {
                remaining: Duration::from_secs(20)
            })
        );

        clock.advance(Duration::from_secs(20));
        assert_eq!(
            state.confirm_end_early("   ", clock.now()),
            Err(SessionError::ReasonRequired)
        );
        state
            .confirm_end_early("  meeting moved  ", clock.now())
            .unwrap();
        assert_eq!(
            state.ended().unwrap().reason(),
            &EndReason::EndedEarly {
                reason: Some("meeting moved".into())
            }
        );
    }

    #[test]
    fn cancelling_a_deep_request_restarts_the_wait() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Deep, &clock);
        state.request_end_early(clock.now()).unwrap();
        clock.advance(Duration::from_secs(40));
        state.cancel_end_request().unwrap();
        assert_eq!(
            state.confirm_end_early("done", clock.now()),
            Err(SessionError::NoEndRequest)
        );
        assert_eq!(
            state.request_end_early(clock.now()),
            Ok(EndEarlyResponse::WaitUntil(
                clock.now().saturating_add(Strictness::DEEP_END_WAIT)
            ))
        );
    }

    #[test]
    fn locked_cannot_end_early() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Locked, &clock);
        assert_eq!(
            state.request_end_early(clock.now()),
            Err(SessionError::Locked)
        );
        assert_eq!(
            state.confirm_end_early("please", clock.now()),
            Err(SessionError::Locked)
        );
        assert!(state.running().is_some());
    }

    #[test]
    fn end_requests_after_the_timer_see_a_completed_session() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Deep, &clock);
        state.request_end_early(clock.now()).unwrap();
        clock.advance(HOUR);
        assert_eq!(
            state.confirm_end_early("done", clock.now()),
            Err(SessionError::NotRunning)
        );
        assert_eq!(state.ended().unwrap().reason(), &EndReason::Completed);
    }

    #[test]
    fn transitions_need_a_running_session() {
        let clock = FakeClock::default();
        let mut state = SessionState::default();
        assert_eq!(
            state.request_end_early(clock.now()),
            Err(SessionError::NotRunning)
        );
        assert_eq!(state.cancel_end_request(), Err(SessionError::NotRunning));
        assert!(!state.tick(clock.now()));
    }

    #[test]
    fn override_policy_follows_strictness() {
        let clock = FakeClock::default();
        for strictness in Strictness::ALL {
            let state = started(strictness, &clock);
            assert_eq!(
                state.running().unwrap().override_policy(),
                strictness.override_policy()
            );
        }
    }

    #[test]
    fn survives_a_json_round_trip() {
        let clock = FakeClock::default();
        let mut state = started(Strictness::Deep, &clock);
        state.request_end_early(clock.now()).unwrap();
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<SessionState>(&json).unwrap(), state);
    }
}
