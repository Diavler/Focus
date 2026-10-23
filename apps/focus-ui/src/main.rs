//! Iced desktop UI. For now a demo that runs sessions in-process (ADR 0002); from build step 3 it
//! talks only to `focus-daemon` over IPC.

mod demo;

use std::time::Duration;

use focus_core::ports::Clock;
use focus_core::{
    BlockConfig, EndEarlyPolicy, EndReason, EndedSession, RunningSession, SessionSpec,
    SessionState, Strictness, Timestamp,
};
use iced::widget::{
    Column, button, column, container, pick_list, radio, row, slider, text, text_input,
};
use iced::{Element, Length, Subscription};

use demo::SystemClock;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("Focus (demo)")
        .subscription(App::subscription)
        .window_size((520.0, 680.0))
        .centered()
        .run()
}

/// Shortest and longest session the duration slider offers, in minutes.
const MINUTES: std::ops::RangeInclusive<u16> = 1..=180;

/// Everything the window shows.
struct App {
    clock: SystemClock,
    configs: Vec<BlockConfig>,
    config_names: Vec<String>,
    selected: Option<String>,
    strictness: Strictness,
    minutes: u16,
    session: SessionState,
    /// Read from the clock at each update, so `view` stays a pure function of the state.
    now: Timestamp,
    reason: String,
    probe: String,
    error: Option<String>,
}

/// User actions and timer ticks.
#[derive(Debug, Clone)]
enum Message {
    ConfigPicked(String),
    StrictnessPicked(Strictness),
    MinutesChanged(u16),
    Start,
    Tick,
    EndEarly,
    ReasonChanged(String),
    ConfirmEnd,
    CancelEnd,
    ProbeChanged(String),
}

impl App {
    fn new() -> Self {
        let clock = SystemClock;
        let (configs, error) = match demo::demo_configs() {
            Ok(configs) => (configs, None),
            Err(error) => (
                Vec::new(),
                Some(format!("Demo configs are invalid: {error}")),
            ),
        };
        let config_names: Vec<String> = configs.iter().map(|config| config.name.clone()).collect();
        Self {
            now: clock.now(),
            clock,
            selected: config_names.first().cloned(),
            config_names,
            configs,
            strictness: Strictness::Deep,
            minutes: 25,
            session: SessionState::default(),
            reason: String::new(),
            probe: String::new(),
            error,
        }
    }

    fn update(&mut self, message: Message) {
        self.now = self.clock.now();
        self.session.tick(self.now);
        if matches!(message, Message::Tick) {
            return;
        }
        let result = match message {
            Message::ConfigPicked(name) => {
                self.selected = Some(name);
                Ok(())
            }
            Message::StrictnessPicked(strictness) => {
                self.strictness = strictness;
                Ok(())
            }
            Message::MinutesChanged(minutes) => {
                self.minutes = minutes;
                Ok(())
            }
            Message::Start => self.start(),
            Message::Tick => Ok(()),
            Message::EndEarly => self
                .session
                .request_end_early(self.now)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Message::ReasonChanged(reason) => {
                self.reason = reason;
                Ok(())
            }
            Message::ConfirmEnd => self
                .session
                .confirm_end_early(&self.reason, self.now)
                .map_err(|error| error.to_string()),
            Message::CancelEnd => {
                self.reason.clear();
                self.session
                    .cancel_end_request()
                    .map_err(|error| error.to_string())
            }
            Message::ProbeChanged(probe) => {
                self.probe = probe;
                Ok(())
            }
        };
        self.error = result.err();
    }

    fn start(&mut self) -> Result<(), String> {
        let config = self
            .selected_config()
            .cloned()
            .ok_or("Pick a configuration first.")?;
        let spec = SessionSpec {
            config,
            strictness: self.strictness,
            duration: Duration::from_secs(u64::from(self.minutes) * 60),
        };
        self.reason.clear();
        self.session
            .start(spec, self.now)
            .map_err(|error| error.to_string())
    }

    /// Ticks once a second while a session runs, to update the countdowns.
    fn subscription(&self) -> Subscription<Message> {
        if self.session.running().is_some() {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let banner = text(
            "Demo build: nothing is blocked yet, and closing this window ends the session. \
             Sessions move to the background service in a later build step.",
        )
        .size(13)
        .style(text::secondary);
        let body = match self.session.running() {
            Some(running) => self.running_view(running),
            None => self.setup_view(),
        };
        let mut page = column![banner, body, self.probe_view()].spacing(28);
        if let Some(error) = &self.error {
            page = page.push(text(error).style(text::danger));
        }
        container(page)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn setup_view(&self) -> Column<'_, Message> {
        let config = row![
            text("Configuration"),
            pick_list(
                self.config_names.as_slice(),
                self.selected.as_ref(),
                Message::ConfigPicked
            ),
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
        let start = button("Start session")
            .style(button::success)
            .on_press_maybe(self.selected.is_some().then_some(Message::Start));

        let mut setup = column![text("New session").size(24), config].spacing(18);
        if let Some(summary) = self.selected_config().map(config_summary) {
            setup = setup.push(text(summary).size(13).style(text::secondary));
        }
        setup = setup.push(strictness).push(duration).push(start);
        if let Some(ended) = self.session.ended() {
            setup = setup.push(text(last_session_summary(ended)));
        }
        setup
    }

    fn running_view<'a>(&'a self, running: &'a RunningSession) -> Column<'a, Message> {
        let spec = running.spec();
        column![
            text(format!(
                "Focusing: {} · {}",
                spec.config.name, spec.strictness
            ))
            .size(20),
            text(format_duration(running.time_left(self.now))).size(64),
            self.end_controls(running),
        ]
        .spacing(16)
    }

    /// End-early controls, which depend on the session's strictness.
    fn end_controls<'a>(&'a self, running: &'a RunningSession) -> Element<'a, Message> {
        match running.spec().strictness.end_early_policy() {
            EndEarlyPolicy::Immediate => button("End session")
                .style(button::danger)
                .on_press(Message::EndEarly)
                .into(),
            EndEarlyPolicy::Forbidden => text("Locked: this session can't be ended early.")
                .style(text::secondary)
                .into(),
            EndEarlyPolicy::WaitAndReason { .. } => match running.end_allowed_at() {
                None => button("Ask to end early")
                    .style(button::secondary)
                    .on_press(Message::EndEarly)
                    .into(),
                Some(allowed_at) => {
                    self.deep_end_form(allowed_at.saturating_duration_since(self.now))
                }
            },
        }
    }

    /// The wait countdown and reason box for ending a Deep session.
    fn deep_end_form(&self, wait: Duration) -> Element<'_, Message> {
        let ready = wait.is_zero() && !self.reason.trim().is_empty();
        let confirm = ready.then_some(Message::ConfirmEnd);
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
                button("Keep focusing").on_press(Message::CancelEnd),
            ]
            .spacing(12),
        ]
        .spacing(10)
        .into()
    }

    /// A box to try URLs against the running (or selected) configuration.
    fn probe_view(&self) -> Column<'_, Message> {
        let config = self
            .session
            .running()
            .map(|running| &running.spec().config)
            .or_else(|| self.selected_config());
        let result = match config {
            Some(config) if !self.probe.trim().is_empty() => {
                format!("{}: {}", config.name, config.verdict(&self.probe))
            }
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
        format!("closes {} (not in the demo)", config.apps.join(", "))
    };
    format!("{} · {} rules · {apps}", config.mode, config.rules.len())
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
    use super::*;

    #[test]
    fn formats_countdowns() {
        assert_eq!(format_duration(Duration::ZERO), "0:00");
        assert_eq!(format_duration(Duration::from_millis(1)), "0:01");
        assert_eq!(format_duration(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_duration(Duration::from_secs(3600 + 62)), "1:01:02");
    }

    #[test]
    fn deep_end_flow_through_messages() {
        let mut app = App::new();
        app.update(Message::StrictnessPicked(Strictness::Deep));
        app.update(Message::Start);
        assert!(app.session.running().is_some());

        app.update(Message::EndEarly);
        app.update(Message::ReasonChanged("done early".into()));
        app.update(Message::ConfirmEnd);
        assert!(
            app.error.as_deref().is_some_and(|e| e.contains("wait")),
            "confirming during the wait is refused: {:?}",
            app.error
        );
        assert!(app.session.running().is_some());

        app.update(Message::CancelEnd);
        assert_eq!(app.error, None);
        assert!(app.reason.is_empty());
    }
}
