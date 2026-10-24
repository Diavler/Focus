//! Starting sessions from the plan: a calendar event whose title has a keyword, or a timed task
//! with a label, runs the configuration its `[[auto_start]]` rule names.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::plan::{CalendarEvent, PlanCache, Task};
use crate::session::{EndReason, EndedSession, SessionState};
use crate::strictness::Strictness;
use crate::time::Timestamp;

/// Maps calendar keywords and task labels to a configuration and strictness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoStartRule {
    /// The configuration to run.
    pub config: String,
    /// How strict the session is.
    pub strictness: Strictness,
    /// An event whose title contains one of these (ignoring case) starts a session.
    pub calendar_keywords: Vec<String>,
    /// A task due at a time, with one of these labels (ignoring case), starts a session.
    pub task_labels: Vec<String>,
    /// How long a task's session lasts when the task has no duration of its own.
    pub task_duration: Duration,
}

impl AutoStartRule {
    /// Session length for tasks without a duration, unless the rule says otherwise.
    pub const DEFAULT_TASK_DURATION: Duration = Duration::from_secs(50 * 60);
}

/// Where a planned session comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSource {
    /// A calendar event.
    Calendar,
    /// A task.
    Task,
}

/// A session the plan says to run, from one event or task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedStart {
    /// Identifies this occurrence, so it starts at most once.
    pub key: String,
    /// Whether it comes from the calendar or a task.
    pub source: PlanSource,
    /// The event's or task's title.
    pub title: String,
    /// The configuration to run.
    pub config: String,
    /// How strict the session is.
    pub strictness: Strictness,
    /// When the session should start.
    pub starts_at: Timestamp,
    /// When it should end.
    pub ends_at: Timestamp,
}

impl PlannedStart {
    /// How long a session started at `now` should run: until `ends_at`, capped at
    /// [`SessionState::MAX_DURATION`].
    pub fn duration_from(&self, now: Timestamp) -> Duration {
        self.ends_at
            .saturating_duration_since(now)
            .min(SessionState::MAX_DURATION)
    }
}

/// Every session the cached plan asks for, sorted by start. Each event or task follows the first
/// rule that matches it.
pub fn planned_starts(rules: &[AutoStartRule], plan: &PlanCache) -> Vec<PlannedStart> {
    let events = plan
        .events()
        .iter()
        .filter_map(|event| from_event(rules, event));
    let tasks = plan
        .tasks()
        .iter()
        .filter_map(|task| from_task(rules, task));
    let mut planned: Vec<PlannedStart> = events.chain(tasks).collect();
    planned.sort_by(|a, b| (a.starts_at, &a.key).cmp(&(b.starts_at, &b.key)));
    planned
}

/// Configurations with a planned session overlapping the next `horizon`, so edits to them cool
/// off like edits to a running session's.
pub fn scheduled_configs(planned: &[PlannedStart], now: Timestamp, horizon: Duration) -> Vec<&str> {
    let until = now.saturating_add(horizon);
    let mut names: Vec<&str> = planned
        .iter()
        .filter(|start| start.ends_at > now && start.starts_at < until)
        .map(|start| start.config.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn from_event(rules: &[AutoStartRule], event: &CalendarEvent) -> Option<PlannedStart> {
    if event.all_day {
        return None;
    }
    let title = event.title.to_lowercase();
    let rule = rules.iter().find(|rule| {
        rule.calendar_keywords
            .iter()
            .any(|keyword| title.contains(&keyword.to_lowercase()))
    })?;
    Some(PlannedStart {
        key: format!("calendar:{}", event.id),
        source: PlanSource::Calendar,
        title: event.title.clone(),
        config: rule.config.clone(),
        strictness: rule.strictness,
        starts_at: event.start,
        ends_at: event.end,
    })
}

fn from_task(rules: &[AutoStartRule], task: &Task) -> Option<PlannedStart> {
    let due_at = task.due_at?;
    let rule = rules.iter().find(|rule| {
        rule.task_labels
            .iter()
            .any(|wanted| task.labels.iter().any(|label| label_eq(label, wanted)))
    })?;
    let duration = task.duration.unwrap_or(rule.task_duration);
    Some(PlannedStart {
        // A recurring task keeps its ID, so the due time tells occurrences apart.
        key: format!("task:{}@{}", task.id, due_at.as_millis()),
        source: PlanSource::Task,
        title: task.title.clone(),
        config: rule.config.clone(),
        strictness: rule.strictness,
        starts_at: due_at,
        ends_at: due_at.saturating_add(duration),
    })
}

/// Labels match ignoring case (in any script) and a leading `@`.
fn label_eq(a: &str, b: &str) -> bool {
    a.trim_start_matches('@').to_lowercase() == b.trim_start_matches('@').to_lowercase()
}

/// Planned sessions that have already started, so ending one early doesn't start it again.
/// Persisted with the state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoStartLog {
    started: Vec<StartedEntry>,
}

/// One planned session that started, remembered until its planned end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StartedEntry {
    key: String,
    until: Timestamp,
    /// When it was started (or given up on).
    at: Timestamp,
}

impl AutoStartLog {
    /// A planned session shorter than this from now isn't worth starting.
    pub const MIN_REMAINING: Duration = Duration::from_secs(60);

    /// The first planned session that should be running at `now` and hasn't started yet.
    pub fn due<'a>(&self, planned: &'a [PlannedStart], now: Timestamp) -> Option<&'a PlannedStart> {
        self.upcoming(planned, now)
            .into_iter()
            .find(|start| start.starts_at <= now)
    }

    /// Planned sessions that haven't started and still have time left to run, soonest first.
    pub fn upcoming<'a>(
        &self,
        planned: &'a [PlannedStart],
        now: Timestamp,
    ) -> Vec<&'a PlannedStart> {
        planned
            .iter()
            .filter(|start| {
                start.ends_at.saturating_duration_since(now) >= Self::MIN_REMAINING
                    && !self.has_started(&start.key)
            })
            .collect()
    }

    /// Remembers that `start` started (or was given up on) at `now`.
    pub fn mark(&mut self, start: &PlannedStart, now: Timestamp) {
        if !self.has_started(&start.key) {
            self.started.push(StartedEntry {
                key: start.key.clone(),
                until: start.ends_at,
                at: now,
            });
        }
    }

    /// After an automatic session is ended early, gives up on every other planned session that
    /// was already under way at that moment (say, a calendar event and a task for the same
    /// block), so ending early sticks. Planned sessions starting later still start. A session
    /// started by hand doesn't count. Returns whether anything changed.
    pub fn end_overlapping(&mut self, planned: &[PlannedStart], ended: &EndedSession) -> bool {
        let early = matches!(ended.reason(), EndReason::EndedEarly { .. });
        let automatic = self
            .started
            .iter()
            .any(|entry| entry.at == ended.started_at());
        if !early || !automatic {
            return false;
        }
        let at = ended.ended_at();
        let mut changed = false;
        for start in planned {
            if start.starts_at <= at && at < start.ends_at && !self.has_started(&start.key) {
                self.mark(start, at);
                changed = true;
            }
        }
        changed
    }

    /// Forgets planned sessions that have ended by `now`. Returns whether anything was forgotten.
    pub fn prune(&mut self, now: Timestamp) -> bool {
        let before = self.started.len();
        self.started.retain(|entry| entry.until > now);
        self.started.len() != before
    }

    /// Whether the planned session with this key has started.
    pub fn has_started(&self, key: &str) -> bool {
        self.started.iter().any(|entry| entry.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Fetched;
    use crate::ports::Clock;
    use crate::testing::FakeClock;

    const MINUTE: Duration = Duration::from_secs(60);

    fn rule(config: &str, keywords: &[&str], labels: &[&str]) -> AutoStartRule {
        AutoStartRule {
            config: config.into(),
            strictness: Strictness::Deep,
            calendar_keywords: keywords.iter().map(|k| (*k).into()).collect(),
            task_labels: labels.iter().map(|l| (*l).into()).collect(),
            task_duration: AutoStartRule::DEFAULT_TASK_DURATION,
        }
    }

    fn event(id: &str, title: &str, start: Timestamp, minutes: u64) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            title: title.into(),
            start,
            end: start.saturating_add(MINUTE * u32::try_from(minutes).unwrap()),
            all_day: false,
        }
    }

    fn task(id: &str, labels: &[&str], due_at: Option<Timestamp>) -> Task {
        Task {
            id: id.into(),
            title: format!("task {id}"),
            labels: labels.iter().map(|l| (*l).into()).collect(),
            due_date: None,
            due_at,
            duration: None,
        }
    }

    fn plan(events: Vec<CalendarEvent>, tasks: Vec<Task>) -> PlanCache {
        let at = Timestamp::from_millis(0);
        PlanCache {
            calendar: Some(Fetched { at, items: events }),
            tasks: Some(Fetched { at, items: tasks }),
        }
    }

    #[test]
    fn keywords_match_titles_ignoring_case() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &["#Focus"], &[])];
        let cache = plan(
            vec![
                event("a", "Deep work #focus", now, 60),
                event("b", "Lunch", now, 60),
            ],
            vec![],
        );
        let planned = planned_starts(&rules, &cache);
        let [only] = planned.as_slice() else {
            panic!("expected one planned start, got {planned:?}");
        };
        assert_eq!(only.key, "calendar:a");
        assert_eq!(only.config, "coding");
        assert_eq!(only.source, PlanSource::Calendar);
        assert_eq!(
            (only.starts_at, only.ends_at),
            (now, now.saturating_add(MINUTE * 60))
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let now = FakeClock::default().now();
        let rules = [
            rule("writing", &["draft"], &[]),
            rule("coding", &["draft", "code"], &[]),
        ];
        let cache = plan(vec![event("a", "Draft and code", now, 30)], vec![]);
        assert_eq!(planned_starts(&rules, &cache)[0].config, "writing");
    }

    #[test]
    fn timed_tasks_with_a_label_start_for_their_duration() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &[], &["@deep"])];
        let mut with_duration = task("1", &["Deep"], Some(now));
        with_duration.duration = Some(MINUTE * 25);
        let cache = plan(
            vec![],
            vec![
                with_duration,
                task("2", &["deep"], Some(now.saturating_add(MINUTE))),
                task("3", &["deep"], None),
                task("4", &["errand"], Some(now)),
            ],
        );
        let planned = planned_starts(&rules, &cache);
        assert_eq!(
            planned.len(),
            2,
            "untimed and unlabelled tasks don't start: {planned:?}"
        );
        assert_eq!(planned[0].ends_at, now.saturating_add(MINUTE * 25));
        assert_eq!(
            planned[1].ends_at,
            now.saturating_add(MINUTE + AutoStartRule::DEFAULT_TASK_DURATION),
            "falls back to the rule's task duration"
        );
        assert_eq!(
            planned[1].key,
            format!("task:2@{}", now.as_millis() + 60_000)
        );
    }

    #[test]
    fn planned_starts_are_sorted_by_start() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &["focus"], &["deep"])];
        let later = now.saturating_add(MINUTE * 90);
        let cache = plan(
            vec![event("late", "focus", later, 30)],
            vec![task("early", &["deep"], Some(now))],
        );
        let starts: Vec<_> = planned_starts(&rules, &cache)
            .into_iter()
            .map(|start| start.starts_at)
            .collect();
        assert_eq!(starts, [now, later]);
    }

    #[test]
    fn a_planned_session_is_due_once_while_it_runs() {
        let clock = FakeClock::default();
        let rules = [rule("coding", &["focus"], &[])];
        let start = clock.now().saturating_add(MINUTE * 10);
        let cache = plan(vec![event("a", "focus", start, 60)], vec![]);
        let planned = planned_starts(&rules, &cache);
        let mut log = AutoStartLog::default();

        assert_eq!(log.due(&planned, clock.now()), None, "not before it starts");
        clock.advance(MINUTE * 30);
        let due = log.due(&planned, clock.now()).expect("due once started");
        assert_eq!(
            due.duration_from(clock.now()),
            MINUTE * 40,
            "runs only what's left"
        );
        log.mark(due, clock.now());
        assert_eq!(
            log.due(&planned, clock.now()),
            None,
            "not again after starting"
        );

        clock.advance(MINUTE * 40);
        assert!(log.prune(clock.now()), "forgotten once it ends");
        assert!(!log.has_started("calendar:a"));
    }

    #[test]
    fn nearly_over_sessions_are_skipped() {
        let clock = FakeClock::default();
        let rules = [rule("coding", &["focus"], &[])];
        let cache = plan(vec![event("a", "focus", clock.now(), 10)], vec![]);
        let planned = planned_starts(&rules, &cache);
        let log = AutoStartLog::default();
        clock.advance(MINUTE * 9 + Duration::from_secs(30));
        assert_eq!(log.due(&planned, clock.now()), None);
    }

    #[test]
    fn long_events_are_capped_at_the_longest_session() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &["focus"], &[])];
        let cache = plan(vec![event("a", "focus all day", now, 24 * 60)], vec![]);
        let planned = planned_starts(&rules, &cache);
        assert_eq!(planned[0].duration_from(now), SessionState::MAX_DURATION);
    }

    #[test]
    fn scheduled_configs_cover_the_horizon() {
        let now = FakeClock::default().now();
        let rules = [
            rule("coding", &["code"], &[]),
            rule("writing", &["write"], &[]),
            rule("reading", &["read"], &[]),
        ];
        let cache = plan(
            vec![
                event("1", "code", now.saturating_add(MINUTE * 60), 30),
                event("2", "code again", now.saturating_add(MINUTE * 120), 30),
                event("3", "write", now.saturating_add(MINUTE * 60 * 30), 30),
                event(
                    "4",
                    "read",
                    Timestamp::from_millis(now.as_millis() - 3_600_000),
                    30,
                ),
            ],
            vec![],
        );
        let planned = planned_starts(&rules, &cache);
        assert_eq!(
            scheduled_configs(&planned, now, MINUTE * 60 * 24),
            ["coding"],
            "not past events or ones beyond the horizon"
        );
    }

    #[test]
    fn labels_ignore_case_in_any_script() {
        assert!(label_eq("@Глубоко", "глубоко"));
        assert!(label_eq("Über", "über"));
        assert!(!label_eq("deep", "deeper"));
    }

    #[test]
    fn all_day_events_never_start_sessions() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &["focus"], &[])];
        let mut day = event("a", "focus day", now, 24 * 60);
        day.all_day = true;
        assert_eq!(planned_starts(&rules, &plan(vec![day], vec![])), []);
    }

    #[test]
    fn ending_an_automatic_session_early_gives_up_overlapping_ones() {
        let clock = FakeClock::default();
        let rules = [rule("coding", &["focus"], &["deep"])];
        let now = clock.now();
        let later = now.saturating_add(MINUTE * 120);
        let cache = plan(
            vec![event("a", "focus", now, 60), event("b", "focus", later, 60)],
            vec![task("t", &["deep"], Some(now.saturating_add(MINUTE * 5)))],
        );
        let planned = planned_starts(&rules, &cache);
        let mut log = AutoStartLog::default();
        let first = log.due(&planned, now).unwrap().clone();
        log.mark(&first, now);
        let mut session = SessionState::default();
        let spec = crate::session::SessionSpec {
            config: crate::config::BlockConfig::new("coding", crate::config::Mode::Blocklist),
            strictness: Strictness::Light,
            duration: first.duration_from(now),
        };
        session.start(spec, now).unwrap();

        clock.advance(MINUTE * 10);
        session.request_end_early(clock.now()).unwrap();
        assert!(log.end_overlapping(&planned, session.ended().unwrap()));
        assert_eq!(
            log.due(&planned, clock.now()),
            None,
            "the overlapping task is given up"
        );
        assert!(
            !log.end_overlapping(&planned, session.ended().unwrap()),
            "only once"
        );
        let upcoming: Vec<&str> = log
            .upcoming(&planned, clock.now())
            .iter()
            .map(|start| start.key.as_str())
            .collect();
        assert_eq!(upcoming, ["calendar:b"], "later blocks still start");
    }

    #[test]
    fn ending_a_manual_session_early_gives_up_nothing() {
        let clock = FakeClock::default();
        let rules = [rule("coding", &["focus"], &[])];
        let cache = plan(vec![event("a", "focus", clock.now(), 60)], vec![]);
        let planned = planned_starts(&rules, &cache);
        let mut log = AutoStartLog::default();
        let mut session = SessionState::default();
        let spec = crate::session::SessionSpec {
            config: crate::config::BlockConfig::new("writing", crate::config::Mode::Blocklist),
            strictness: Strictness::Light,
            duration: MINUTE * 30,
        };
        session.start(spec, clock.now()).unwrap();
        clock.advance(MINUTE);
        session.request_end_early(clock.now()).unwrap();
        assert!(!log.end_overlapping(&planned, session.ended().unwrap()));
        assert!(log.due(&planned, clock.now()).is_some());
    }

    #[test]
    fn the_log_survives_a_json_round_trip() {
        let now = FakeClock::default().now();
        let rules = [rule("coding", &["focus"], &[])];
        let cache = plan(vec![event("a", "focus", now, 60)], vec![]);
        let mut log = AutoStartLog::default();
        log.mark(&planned_starts(&rules, &cache)[0], now);
        let json = serde_json::to_string(&log).unwrap();
        assert_eq!(serde_json::from_str::<AutoStartLog>(&json).unwrap(), log);
    }
}
