//! Iced desktop UI. Talks only to `focus-daemon` over IPC, so closing the window never ends a session.

mod daemon;

use std::path::Path;
use std::time::Duration;

use focus_core::{
    BlockConfig, EndEarlyPolicy, EndReason, EndedSession, RunningSession, Strictness,
};
use iced::widget::{
    Column, button, column, container, pick_list, radio, row, slider, text, text_input,
};
use iced::{Element, Length, Subscription, Task};
use ipc_proto::{Request, StatusReport};
use ipc_transport::Endpoint;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("Focus")
        .subscription(App::subscription)
        .window_size((520.0, 720.0))
        .centered()
        .run()
}

/// Shortest and longest session the duration slider offers, in minutes.
const MINUTES: std::ops::RangeInclusive<u16> = 1..=180;

/// How often the window asks the daemon for its status.
const POLL: Duration = Duration::from_secs(1);

/// Configurations are re-fetched every this many polls, to pick up settings edits.
const CONFIGS_EVERY_POLLS: u32 = 10;

/// Everything the window shows.
struct App {
    endpoint: Endpoint,
    /// The last status the daemon sent; `None` until it first answers.
    status: Option<StatusReport>,
    /// Why the daemon can't be reached, if it can't.
    unreachable: Option<String>,
    configs: Vec<BlockConfig>,
    config_names: Vec<String>,
    selected: Option<String>,
    strictness: Strictness,
    minutes: u16,
    reason: String,
    probe: String,
    probe_result: Option<String>,
    /// What the last URL check asked; a new check is sent when this would change.
    probe_asked: Option<ProbeKey>,
    error: Option<String>,
    polls: u32,
    /// A status request is on its way, so polls don't pile up if the daemon is slow.
    status_pending: bool,
    /// A configs request is on its way.
    configs_pending: bool,
    /// An action (start, end, ...) is on its way; its buttons are disabled until it answers.
    action_pending: bool,
}

/// One URL check: the URL and what it was checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeKey {
    url: String,
    /// The running session's config, if a session was running.
    session: Option<String>,
    /// The config picked in the setup view, used when no session is running.
    selected: Option<String>,
}

/// User actions, timer ticks and daemon answers.
#[derive(Debug, Clone)]
enum Message {
    Poll,
    StatusLoaded(Result<StatusReport, String>),
    ConfigsLoaded(Result<Vec<BlockConfig>, String>),
    ConfigPicked(String),
    StrictnessPicked(Strictness),
    MinutesChanged(u16),
    Start,
    EndEarly,
    ReasonChanged(String),
    ConfirmEnd,
    CancelEnd,
    ActionDone(Result<(), String>),
    ProbeChanged(String),
    ProbeChecked {
        key: ProbeKey,
        result: Result<String, String>,
    },
}

impl App {
    fn new() -> (Self, Task<Message>) {
        // The daemon's endpoint depends on the install folder, which holds both executables.
        let install = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let mut app = Self {
            endpoint: Endpoint::for_install(&install),
            status: None,
            unreachable: None,
            configs: Vec::new(),
            config_names: Vec::new(),
            selected: None,
            strictness: Strictness::Deep,
            minutes: 25,
            reason: String::new(),
            probe: String::new(),
            probe_result: None,
            probe_asked: None,
            error: None,
            polls: 0,
            status_pending: false,
            configs_pending: false,
            action_pending: false,
        };
        let boot = Task::batch([app.fetch_status(), app.fetch_configs()]);
        (app, boot)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Poll => {
                self.polls = self.polls.wrapping_add(1);
                let mut tasks = Vec::new();
                if !self.status_pending {
                    tasks.push(self.fetch_status());
                }
                let configs_due =
                    self.polls.is_multiple_of(CONFIGS_EVERY_POLLS) || self.configs.is_empty();
                if configs_due && !self.configs_pending {
                    tasks.push(self.fetch_configs());
                }
                return Task::batch(tasks);
            }
            Message::StatusLoaded(result) => {
                self.status_pending = false;
                match result {
                    Ok(status) => {
                        self.status = Some(status);
                        self.unreachable = None;
                    }
                    Err(problem) => self.unreachable = Some(problem),
                }
                return self.check_probe();
            }
            Message::ConfigsLoaded(result) => {
                self.configs_pending = false;
                match result {
                    Ok(configs) => {
                        self.set_configs(configs);
                        // The rules may have changed, so check the URL again.
                        self.probe_asked = None;
                    }
                    Err(problem) => self.unreachable = Some(problem),
                }
                return self.check_probe();
            }
            Message::ConfigPicked(name) => {
                self.selected = Some(name);
                return self.check_probe();
            }
            Message::StrictnessPicked(strictness) => self.strictness = strictness,
            Message::MinutesChanged(minutes) => self.minutes = minutes,
            Message::Start => {
                let Some(config) = self.selected.clone() else {
                    return Task::none();
                };
                self.reason.clear();
                return self.act(Request::StartSession {
                    config,
                    strictness: self.strictness,
                    duration_secs: u64::from(self.minutes) * 60,
                });
            }
            Message::EndEarly => return self.act(Request::EndSession),
            Message::ReasonChanged(reason) => self.reason = reason,
            Message::ConfirmEnd => {
                return self.act(Request::ConfirmEndSession {
                    reason: self.reason.clone(),
                });
            }
            Message::CancelEnd => {
                self.reason.clear();
                return self.act(Request::CancelEndSession);
            }
            Message::ActionDone(result) => {
                self.action_pending = false;
                self.error = result.err();
                // The URL check follows once the new status arrives.
                return self.fetch_status();
            }
            Message::ProbeChanged(probe) => {
                self.probe = probe;
                return self.check_probe();
            }
            Message::ProbeChecked { key, result } => {
                // Answers can arrive out of order; only show the one for what's on screen now.
                if self.probe_key().as_ref() == Some(&key) {
                    self.probe_result = Some(result.unwrap_or_else(|error| error));
                }
            }
        }
        Task::none()
    }

    fn set_configs(&mut self, configs: Vec<BlockConfig>) {
        self.config_names = configs.iter().map(|config| config.name.clone()).collect();
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|name| self.config_names.contains(name));
        if !still_there {
            self.selected = self.config_names.first().cloned();
        }
        self.configs = configs;
    }

    fn fetch_status(&mut self) -> Task<Message> {
        self.status_pending = true;
        Task::perform(daemon::status(self.endpoint.clone()), Message::StatusLoaded)
    }

    fn fetch_configs(&mut self) -> Task<Message> {
        self.configs_pending = true;
        Task::perform(
            daemon::configs(self.endpoint.clone()),
            Message::ConfigsLoaded,
        )
    }

    fn act(&mut self, request: Request) -> Task<Message> {
        self.action_pending = true;
        Task::perform(
            daemon::act(self.endpoint.clone(), request),
            Message::ActionDone,
        )
    }

    /// What a URL check would ask right now, or `None` if the probe box is empty.
    fn probe_key(&self) -> Option<ProbeKey> {
        let url = self.probe.trim();
        (!url.is_empty()).then(|| ProbeKey {
            url: url.to_owned(),
            session: self
                .running()
                .map(|running| running.spec().config.name.clone()),
            selected: self.selected.clone(),
        })
    }

    /// Asks the daemon what the running session (or, with none, the selected config) does with
    /// the URL in the probe box, unless that exact question was already asked.
    fn check_probe(&mut self) -> Task<Message> {
        let key = self.probe_key();
        if key == self.probe_asked {
            return Task::none();
        }
        self.probe_asked.clone_from(&key);
        let Some(key) = key else {
            return Task::none();
        };
        // With a session running, the daemon checks against the session's own copy of the rules.
        let config = match key.session {
            Some(_) => None,
            None => key.selected.clone(),
        };
        let request = daemon::check(self.endpoint.clone(), key.url.clone(), config);
        Task::perform(request, move |result| Message::ProbeChecked {
            key: key.clone(),
            result,
        })
    }

    /// Polls the daemon once a second.
    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(POLL).map(|_| Message::Poll)
    }

    fn running(&self) -> Option<&RunningSession> {
        self.status.as_ref()?.session.running()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut page = column![].spacing(24);
        match (&self.status, &self.unreachable) {
            (_, Some(problem)) => {
                page = page.push(
                    column![
                        text("Can't reach the Focus daemon.").size(20),
                        text("Start focus-daemon from this folder (`cargo run -p focus-daemon` in development); this window reconnects by itself."),
                        text(problem).size(13).style(text::secondary),
                    ]
                    .spacing(8),
                );
            }
            (None, None) => page = page.push(text("Connecting to the Focus daemon...")),
            (Some(status), None) => {
                if let Some(problem) = &status.settings_problem {
                    page = page.push(
                        text(format!(
                            "Settings problem (the last good settings stay in effect): {problem}"
                        ))
                        .size(13)
                        .style(text::danger),
                    );
                }
                page = page.push(match status.session.running() {
                    Some(running) => self.running_view(running, status),
                    None => self.setup_view(status),
                });
                if let Some(pending) = pending_summary(status) {
                    page = page.push(text(pending).size(13).style(text::secondary));
                }
                page = page.push(self.probe_view());
            }
        }
        if let Some(error) = &self.error {
            page = page.push(text(error).style(text::danger));
        }
        page = page.push(
            text("Sessions run in the background daemon; closing this window doesn't end them. Nothing is blocked yet.")
                .size(12)
                .style(text::secondary),
        );
        container(page)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn setup_view<'a>(&'a self, status: &'a StatusReport) -> Column<'a, Message> {
        let config = row![
            text("Configuration"),
            pick_list(
                self.config_names.as_slice(),
                self.selected.as_ref(),
                Message::ConfigPicked
            )
            .placeholder("no configurations"),
        ]
        .spacing(12);
        let strictness = Strictness::ALL.iter().fold(
            column![text("Strictness")].spacing(8),
            |levels, &level| {
                levels.push(radio(
                    format!("{level}: {}", describe(level)),
                    level,
                    Some(self.strictness),
                    Message::StrictnessPicked,
                ))
            },
        );
        let duration = column![
            text(format!("Duration: {} min", self.minutes)),
            slider(MINUTES, self.minutes, Message::MinutesChanged),
        ]
        .spacing(8);
        let can_start = self.selected.is_some() && !self.action_pending;
        let start = button("Start session")
            .style(button::success)
            .on_press_maybe(can_start.then_some(Message::Start));

        let mut setup = column![text("New session").size(24), config].spacing(18);
        if let Some(summary) = self.selected_config().map(config_summary) {
            setup = setup.push(text(summary).size(13).style(text::secondary));
        }
        setup = setup.push(strictness).push(duration).push(start);
        if let Some(ended) = status.session.ended() {
            setup = setup.push(text(last_session_summary(ended)));
        }
        setup
    }

    fn running_view<'a>(
        &'a self,
        running: &'a RunningSession,
        status: &'a StatusReport,
    ) -> Column<'a, Message> {
        let spec = running.spec();
        column![
            text(format!(
                "Focusing: {} · {}",
                spec.config.name, spec.strictness
            ))
            .size(20),
            text(format_duration(running.time_left(status.now))).size(64),
            self.end_controls(running, status),
        ]
        .spacing(16)
    }

    /// End-early controls, which depend on the session's strictness.
    fn end_controls<'a>(
        &'a self,
        running: &'a RunningSession,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        match running.spec().strictness.end_early_policy() {
            EndEarlyPolicy::Immediate => button("End session")
                .style(button::danger)
                .on_press_maybe(self.idle_then(Message::EndEarly))
                .into(),
            EndEarlyPolicy::Forbidden => text("Locked: this session can't be ended early.")
                .style(text::secondary)
                .into(),
            EndEarlyPolicy::WaitAndReason { .. } => match running.end_allowed_at() {
                None => button("Ask to end early")
                    .style(button::secondary)
                    .on_press_maybe(self.idle_then(Message::EndEarly))
                    .into(),
                Some(allowed_at) => {
                    self.deep_end_form(allowed_at.saturating_duration_since(status.now))
                }
            },
        }
    }

    /// The wait countdown and reason box for ending a Deep session.
    fn deep_end_form(&self, wait: Duration) -> Element<'_, Message> {
        let ready = wait.is_zero() && !self.reason.trim().is_empty();
        let confirm = if ready {
            self.idle_then(Message::ConfirmEnd)
        } else {
            None
        };
        let status = if wait.is_zero() {
            "Why are you ending early?".to_owned()
        } else {
            format!(
                "You can end in {}. Why are you ending early?",
                format_duration(wait)
            )
        };
        column![
            text(status),
            text_input("Type a reason", &self.reason)
                .on_input(Message::ReasonChanged)
                .on_submit_maybe(confirm.clone()),
            row![
                button("End session")
                    .style(button::danger)
                    .on_press_maybe(confirm),
                button("Keep focusing").on_press_maybe(self.idle_then(Message::CancelEnd)),
            ]
            .spacing(12),
        ]
        .spacing(10)
        .into()
    }

    /// A box to try URLs against the running (or selected) configuration.
    fn probe_view(&self) -> Column<'_, Message> {
        let result = match (&self.probe_result, self.probe.trim().is_empty()) {
            (Some(result), false) => result.clone(),
            _ => "Type a URL to see what the configuration would do with it.".to_owned(),
        };
        column![
            text("Check a URL"),
            text_input("reddit.com/r/rust/comments/abc", &self.probe)
                .on_input(Message::ProbeChanged),
            text(result),
        ]
        .spacing(8)
    }

    /// `message`, unless an action is still waiting for the daemon (so double clicks don't send
    /// it twice).
    fn idle_then(&self, message: Message) -> Option<Message> {
        (!self.action_pending).then_some(message)
    }

    fn selected_config(&self) -> Option<&BlockConfig> {
        let name = self.selected.as_ref()?;
        self.configs.iter().find(|config| &config.name == name)
    }
}

/// One line on what a strictness level allows, derived from its policy.
fn describe(strictness: Strictness) -> String {
    match strictness.end_early_policy() {
        EndEarlyPolicy::Immediate => "end any time with one click".to_owned(),
        EndEarlyPolicy::WaitAndReason { wait } => format!(
            "end early after a {}-second wait and a typed reason",
            wait.as_secs()
        ),
        EndEarlyPolicy::Forbidden => "no ending early".to_owned(),
    }
}

/// Mode, rule count and apps for a configuration.
fn config_summary(config: &BlockConfig) -> String {
    let apps = if config.apps.is_empty() {
        "no apps".to_owned()
    } else {
        format!("closes {} (not yet enforced)", config.apps.join(", "))
    };
    format!("{} · {} rules · {apps}", config.mode, config.rules.len())
}

/// Config edits still cooling off, e.g. `Edit to "coding" applies in 23:59:59.`
fn pending_summary(status: &StatusReport) -> Option<String> {
    let lines: Vec<String> = status
        .pending_changes
        .iter()
        .map(|change| {
            let what = if change.deleted {
                "Deleting"
            } else {
                "Edit to"
            };
            let wait = change.apply_after.saturating_duration_since(status.now);
            format!(
                "{what} \"{}\" applies in {} (cooling off).",
                change.config,
                format_duration(wait)
            )
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// How the last session went.
fn last_session_summary(ended: &EndedSession) -> String {
    let how = match ended.reason() {
        EndReason::Completed => "completed".to_owned(),
        EndReason::EndedEarly { reason: None } => "ended early".to_owned(),
        EndReason::EndedEarly {
            reason: Some(reason),
        } => format!("ended early (\"{reason}\")"),
    };
    let spec = ended.spec();
    let length = ended
        .ended_at()
        .saturating_duration_since(ended.started_at());
    format!(
        "Last session: {} · {}, {how} after {}.",
        spec.config.name,
        spec.strictness,
        format_duration(length)
    )
}

/// `m:ss` or `h:mm:ss`, rounding up so a countdown shows `0:00` only when it's over.
fn format_duration(duration: Duration) -> String {
    let total = duration.as_millis().div_ceil(1000);
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use focus_core::{SessionState, Timestamp};
    use ipc_proto::PendingSummary;

    use super::*;

    #[test]
    fn formats_countdowns() {
        assert_eq!(format_duration(Duration::ZERO), "0:00");
        assert_eq!(format_duration(Duration::from_millis(1)), "0:01");
        assert_eq!(format_duration(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_duration(Duration::from_secs(3600 + 62)), "1:01:02");
    }

    #[test]
    fn summarises_pending_changes() {
        let mut status = StatusReport {
            now: Timestamp::from_millis(0),
            session: SessionState::Idle,
            pending_changes: Vec::new(),
            settings_problem: None,
            browsers: Vec::new(),
        };
        assert_eq!(pending_summary(&status), None);
        status.pending_changes = vec![
            PendingSummary {
                config: "coding".into(),
                apply_after: Timestamp::from_millis(90_000),
                deleted: false,
            },
            PendingSummary {
                config: "old".into(),
                apply_after: Timestamp::from_millis(3_600_000),
                deleted: true,
            },
        ];
        assert_eq!(
            pending_summary(&status).unwrap(),
            "Edit to \"coding\" applies in 1:30 (cooling off).\n\
             Deleting \"old\" applies in 1:00:00 (cooling off)."
        );
    }
}
