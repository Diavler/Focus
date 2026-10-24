//! The kill switch: stopping Focus entirely, ending any session. It's an emergency exit, so it
//! always works, but only after a wait (and, during a Locked session, a longer wait plus a typed
//! reason), so it isn't an easy way out of an urge.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::session::RunningSession;
use crate::strictness::Strictness;
use crate::time::Timestamp;

/// How hard stopping Focus is right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopPolicy {
    /// How long after asking the stop can be confirmed.
    pub wait: Duration,
    /// Whether confirming needs a typed reason.
    pub reason_required: bool,
    /// When the running session started, if one is running. The wait never counts time from
    /// before it, so a request left pending while idle can't shorten a session's wait.
    pub since: Option<Timestamp>,
}

impl StopPolicy {
    /// The wait outside Locked sessions.
    pub const WAIT: Duration = Duration::from_secs(60);
    /// The wait during a Locked session.
    pub const LOCKED_WAIT: Duration = Duration::from_secs(5 * 60);

    /// The policy while `running` is the running session (if any).
    pub fn for_session(running: Option<&RunningSession>) -> Self {
        let since = running.map(RunningSession::started_at);
        match running.map(|running| running.spec().strictness) {
            Some(Strictness::Locked) => Self {
                wait: Self::LOCKED_WAIT,
                reason_required: true,
                since,
            },
            _ => Self {
                wait: Self::WAIT,
                reason_required: false,
                since,
            },
        }
    }

    /// When a request made at `requested` may be confirmed.
    fn allowed_at(&self, requested: Timestamp) -> Timestamp {
        let counts_from = self.since.map_or(requested, |since| since.max(requested));
        counts_from.saturating_add(self.wait)
    }
}

/// Why a stop was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopError {
    /// Confirming without asking first.
    #[error("ask to stop Focus before confirming")]
    NotRequested,
    /// The wait isn't over.
    #[error("wait {remaining:?} more before stopping Focus")]
    StillWaiting {
        /// Time left to wait.
        remaining: Duration,
    },
    /// A Locked session is running, so a reason is needed.
    #[error("type a reason to stop Focus during a locked session")]
    ReasonRequired,
}

/// A pending request to stop Focus. Not persisted: a restart only starts a fresh wait.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KillSwitch {
    requested_at: Option<Timestamp>,
}

impl KillSwitch {
    /// Asks to stop at `now` and returns when it may be confirmed. Asking again keeps the first
    /// request's time.
    pub fn request(&mut self, now: Timestamp, policy: StopPolicy) -> Timestamp {
        policy.allowed_at(*self.requested_at.get_or_insert(now))
    }

    /// Withdraws the request; the next one waits from the start.
    pub fn cancel(&mut self) {
        self.requested_at = None;
    }

    /// When a pending request may be confirmed under `policy`, if there is one.
    pub fn allowed_at(&self, policy: StopPolicy) -> Option<Timestamp> {
        self.requested_at
            .map(|requested| policy.allowed_at(requested))
    }

    /// Checks that stopping is allowed at `now`. `policy` is the one in force now, so a session
    /// that started during the wait restarts it (with a Locked session's longer wait).
    pub fn confirm(
        &self,
        now: Timestamp,
        reason: &str,
        policy: StopPolicy,
    ) -> Result<(), StopError> {
        let allowed_at = self.allowed_at(policy).ok_or(StopError::NotRequested)?;
        if now < allowed_at {
            return Err(StopError::StillWaiting {
                remaining: allowed_at.saturating_duration_since(now),
            });
        }
        if policy.reason_required && reason.trim().is_empty() {
            return Err(StopError::ReasonRequired);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BlockConfig, Mode};
    use crate::ports::Clock;
    use crate::session::{SessionSpec, SessionState};
    use crate::testing::FakeClock;

    fn session(strictness: Strictness, now: Timestamp) -> SessionState {
        let mut state = SessionState::default();
        let spec = SessionSpec {
            config: BlockConfig::new("coding", Mode::Blocklist),
            strictness,
            duration: Duration::from_secs(3600),
        };
        state.start(spec, now).unwrap();
        state
    }

    #[test]
    fn waits_a_minute_then_stops() {
        let clock = FakeClock::default();
        let policy = StopPolicy::for_session(None);
        let mut switch = KillSwitch::default();
        assert_eq!(
            switch.confirm(clock.now(), "", policy),
            Err(StopError::NotRequested)
        );
        let allowed_at = switch.request(clock.now(), policy);
        assert_eq!(allowed_at, clock.now().saturating_add(StopPolicy::WAIT));
        clock.advance(Duration::from_secs(30));
        assert_eq!(
            switch.request(clock.now(), policy),
            allowed_at,
            "asking again keeps the time"
        );
        assert_eq!(
            switch.confirm(clock.now(), "", policy),
            Err(StopError::StillWaiting {
                remaining: Duration::from_secs(30)
            })
        );
        clock.advance(Duration::from_secs(30));
        assert_eq!(switch.confirm(clock.now(), "", policy), Ok(()));
    }

    #[test]
    fn locked_sessions_wait_longer_and_need_a_reason() {
        let clock = FakeClock::default();
        let locked = session(Strictness::Locked, clock.now());
        let policy = StopPolicy::for_session(locked.running());
        assert_eq!(policy.wait, StopPolicy::LOCKED_WAIT);
        let deep = session(Strictness::Deep, clock.now());
        assert!(!StopPolicy::for_session(deep.running()).reason_required);

        let mut switch = KillSwitch::default();
        switch.request(clock.now(), policy);
        clock.advance(StopPolicy::LOCKED_WAIT);
        assert_eq!(
            switch.confirm(clock.now(), "  ", policy),
            Err(StopError::ReasonRequired)
        );
        assert_eq!(
            switch.confirm(clock.now(), "the build is on fire", policy),
            Ok(())
        );
    }

    #[test]
    fn a_request_from_before_a_session_doesnt_shorten_its_wait() {
        let clock = FakeClock::default();
        let mut switch = KillSwitch::default();
        switch.request(clock.now(), StopPolicy::for_session(None));
        clock.advance(Duration::from_secs(3600));
        let locked = session(Strictness::Locked, clock.now());
        let policy = StopPolicy::for_session(locked.running());
        assert_eq!(
            switch.allowed_at(policy),
            Some(clock.now().saturating_add(StopPolicy::LOCKED_WAIT))
        );
        assert_eq!(
            switch.confirm(clock.now(), "a reason", policy),
            Err(StopError::StillWaiting {
                remaining: StopPolicy::LOCKED_WAIT
            })
        );
        clock.advance(StopPolicy::LOCKED_WAIT);
        assert_eq!(switch.confirm(clock.now(), "a reason", policy), Ok(()));
    }

    #[test]
    fn cancelling_restarts_the_wait() {
        let clock = FakeClock::default();
        let policy = StopPolicy::for_session(None);
        let mut switch = KillSwitch::default();
        switch.request(clock.now(), policy);
        clock.advance(StopPolicy::WAIT);
        switch.cancel();
        assert_eq!(switch.allowed_at(policy), None);
        assert_eq!(
            switch.request(clock.now(), policy),
            clock.now().saturating_add(StopPolicy::WAIT)
        );
    }
}
