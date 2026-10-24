//! The Sessions page: the running session (or how the last one went), what starts next, starting a
//! session by hand, and whether browsers and login are protected.

use std::time::Duration;

use chrono::{Local, TimeZone};
use focus_core::{
    BlockConfig, BrowserPolicy, EndEarlyPolicy, EndReason, EndedSession, OverridePolicy,
    RunningSession, Strictness, Timestamp,
};
use iced::widget::{
    Column, button, column, pick_list, radio, row, slider, space, text, text_input,
};
use iced::{Alignment, Element, Length};
use ipc_proto::StatusReport;

use crate::agenda::clock;
use crate::help::info;
use crate::plan_pages::Line;
use crate::style::{Tone, bold, caption, card, dot, heading, hero, pill, title, toned};
use crate::{App, MINUTES, Message};

impl App {
    /// The Sessions page.
    pub(crate) fn sessions_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            title("Sessions"),
            space::horizontal(),
            button(text("Settings").size(15))
                .padding([8, 16])
                .style(button::primary)
                .on_press(Message::OpenSettings),
        ]
        .align_y(Alignment::Center);
        let mut page = column![header].spacing(18);
        if let Some(problem) = &status.settings_problem {
            page = page.push(card(
                toned(
                    format!("Settings problem (the last good settings stay in effect): {problem}"),
                    Tone::Bad,
                )
                .size(14),
            ));
        }
        match status.session.running() {
            Some(running) => page = page.push(self.running_card(running, status)),
            None => {
                page = page.push(self.idle_card(status));
                page = page.push(self.next_start_card(status));
                page = page.push(self.new_session_card());
            }
        }
        if let Some(pending) = pending_summary(status) {
            page = page.push(card(
                column![heading("Cooling off"), caption(pending)].spacing(8),
            ));
        }
        page.push(protection_card(status)).into()
    }

    fn running_card<'a>(
        &'a self,
        running: &'a RunningSession,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        let spec = running.spec();
        hero(
            column![
                row![
                    pill("In session", Tone::Good),
                    bold(format!("{} · {}", spec.config.name, spec.strictness)).size(16),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                bold(format_duration(running.time_left(status.now))).size(64),
                caption(format!("Ends at {}.", clock(running.ends_at(), &Local))),
                caption(unlock_line(running, status)),
                self.end_controls(running, status),
            ]
            .spacing(12),
        )
        .into()
    }

    fn idle_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let mut body = column![heading("No session running")].spacing(8);
        body = body.push(match status.session.ended() {
            Some(ended) => caption(last_session_summary(ended)),
            None => caption("Start one below, or let your calendar and Todoist start them."),
        });
        card(body).into()
    }

    /// The next session the plan will start.
    fn next_start_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let next = self.plan.as_ref().and_then(|plan| plan.upcoming.first());
        let body: Element<'a, Message> = match next {
            Some(start) => column![
                bold(format!(
                    "{} · {}",
                    when_label(start.starts_at, status.now),
                    start.title
                ))
                .size(15),
                caption(format!(
                    "Starts {} ({}) for {}.",
                    start.config,
                    start.strictness,
                    format_duration(start.duration_from(start.starts_at.max(status.now)))
                )),
            ]
            .spacing(4)
            .into(),
            None => caption(
                "Nothing planned. Add automatic starts in Settings to start sessions from your \
                 calendar or Todoist.",
            )
            .into(),
        };
        card(
            column![
                row![
                    heading("Next automatic start"),
                    info(
                        "Sessions your calendar or Todoist will start by themselves, from the \
                         rules under Settings → Automatic starts."
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                body,
            ]
            .spacing(10),
        )
        .into()
    }

    fn new_session_card(&self) -> Element<'_, Message> {
        let config = row![
            text("Configuration").width(Length::Fixed(120.0)),
            pick_list(
                self.config_names.as_slice(),
                self.selected.as_ref(),
                Message::ConfigPicked
            )
            .placeholder("no configurations"),
            info(
                "A named set of rules: which sites are blocked, delayed, asked about or have \
                 their feeds hidden, and which apps are closed. Edit them in Settings → \
                 Configurations."
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let strictness = Strictness::ALL.iter().fold(
            column![
                row![
                    text("Strictness"),
                    info(
                        "How hard the session is to get out of. Pick Deep or Locked when you \
                         know you'll be tempted: the extra friction is the point."
                    ),
                ]
                .spacing(8)
            ]
            .spacing(8),
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
            row![
                text(format!("Duration: {} min", self.minutes)),
                info("How long the session runs. It ends by itself when the time is up."),
            ]
            .spacing(8),
            slider(MINUTES, self.minutes, Message::MinutesChanged),
        ]
        .spacing(8);
        let can_start = self.selected.is_some() && !self.action_pending;
        let start = button(text("Start session").size(15))
            .padding([10, 20])
            .style(button::success)
            .on_press_maybe(can_start.then_some(Message::Start));
        let mut body = column![heading("Start a session"), config].spacing(16);
        if let Some(summary) = self.selected_config().map(config_summary) {
            body = body.push(caption(summary));
        }
        card(body.push(strictness).push(duration).push(start)).into()
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
            EndEarlyPolicy::Forbidden => {
                caption("Locked: this session can't be ended early.").into()
            }
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
        let prompt = if wait.is_zero() {
            "Why are you ending early?".to_owned()
        } else {
            format!(
                "You can end in {}. Why are you ending early?",
                format_duration(wait)
            )
        };
        column![
            text(prompt),
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

    pub(crate) fn selected_config(&self) -> Option<&BlockConfig> {
        let name = self.selected.as_ref()?;
        self.configs.iter().find(|config| &config.name == name)
    }
}

/// Browser protection and start-at-login.
fn protection_card(status: &StatusReport) -> Element<'_, Message> {
    let mut lines: Column<'_, Message> = column![
        row![
            heading("Protection"),
            info(
                "During a session, browsers without the Focus extension (or whose extension \
                 can't run in private windows) are closed, and so are the configuration's apps."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    ]
    .spacing(8);
    for line in browser_lines(status)
        .into_iter()
        .chain([login_line(status)])
    {
        lines = lines.push(
            row![dot(line.tone), toned(line.text, line.tone).size(14)]
                .spacing(8)
                .align_y(Alignment::Center),
        );
    }
    card(lines).into()
}

/// `14:00` today, or `Sun 25 · 14:00`; `Now` if it has started.
fn when_label(at: Timestamp, now: Timestamp) -> String {
    if at <= now {
        return "Now".to_owned();
    }
    let local = |time: Timestamp| {
        let millis = i64::try_from(time.as_millis()).unwrap_or(i64::MAX);
        Local.timestamp_millis_opt(millis).single()
    };
    match (local(at), local(now)) {
        (Some(at), Some(now)) if at.date_naive() == now.date_naive() => {
            at.format("%H:%M").to_string()
        }
        (Some(at), _) => at.format("%a %-d · %H:%M").to_string(),
        _ => "Soon".to_owned(),
    }
}

/// Which browsers are protected, and what to fix for the others.
pub(crate) fn browser_lines(status: &StatusReport) -> Vec<Line> {
    if status.browsers.is_empty() {
        return vec![Line {
            text: "No browser extension is connected, so browsers are closed during sessions."
                .to_owned(),
            tone: Tone::Bad,
        }];
    }
    status
        .browsers
        .iter()
        .map(|browser| {
            let name = browser_name(&browser.browser);
            // The same limit the daemon enforces.
            let stale = status.now.saturating_duration_since(browser.last_seen)
                > BrowserPolicy::default().heartbeat_timeout;
            if stale {
                Line {
                    text: format!("{name}: the extension stopped checking in."),
                    tone: Tone::Bad,
                }
            } else if !browser.incognito_allowed {
                Line {
                    text: format!(
                        "{name}: turn on \"Allow in Incognito\" for Focus (Extensions → Focus → \
                         Details), or {name} is closed during sessions."
                    ),
                    tone: Tone::Bad,
                }
            } else {
                Line {
                    text: format!("{name}: protected."),
                    tone: Tone::Good,
                }
            }
        })
        .collect()
}

/// A browser's heartbeat name as people write it.
fn browser_name(heartbeat: &str) -> String {
    match heartbeat {
        "chrome" => "Chrome".to_owned(),
        "edge" => "Edge".to_owned(),
        "brave" => "Brave".to_owned(),
        "opera" => "Opera".to_owned(),
        "chromium" => "Chromium".to_owned(),
        other => other.to_owned(),
    }
}

/// Whether Focus starts at login, and when a requested switch-off takes effect.
pub(crate) fn login_line(status: &StatusReport) -> Line {
    if let Some(problem) = &status.start_at_login_problem {
        return Line {
            text: format!("Start at login isn't set up: {problem}"),
            tone: Tone::Bad,
        };
    }
    match (status.start_at_login, status.start_at_login_off_after) {
        (true, Some(off_after)) => Line {
            text: format!(
                "Starts at login; turning that off takes effect in {} (cooling off).",
                format_duration(off_after.saturating_duration_since(status.now))
            ),
            tone: Tone::Warn,
        },
        (true, None) => Line {
            text: "Starts at login.".to_owned(),
            tone: Tone::Good,
        },
        (false, _) => Line {
            text: "Doesn't start at login (Settings → General).".to_owned(),
            tone: Tone::Warn,
        },
    }
}

/// One line on what a strictness level allows, derived from its policy.
pub(crate) fn describe(strictness: Strictness) -> String {
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
        format!("closes {}", config.apps.join(", "))
    };
    format!("{} · {} rules · {apps}", config.mode, config.rules.len())
}

/// Config edits still cooling off, e.g. `Edit to "coding" applies in 23:59:59.`
pub(crate) fn pending_summary(status: &StatusReport) -> Option<String> {
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
        .chain(status.friction_loosen_after.map(|at| {
            format!(
                "Looser friction limits apply in {} (cooling off).",
                format_duration(at.saturating_duration_since(status.now))
            )
        }))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// What unlocking a blocked page costs in the running session.
fn unlock_line(running: &RunningSession, status: &StatusReport) -> String {
    let minutes = status.friction.unlock_minutes;
    match running.override_policy() {
        OverridePolicy::Free => {
            format!("Blocked pages can be unlocked for {minutes} min from the browser.")
        }
        OverridePolicy::SpendUnlock => format!(
            "Unlocks left: {} of {} in 24 hours ({minutes} min each, with a reason).",
            status.unlocks_left, status.friction.unlocks_per_day
        ),
        OverridePolicy::Forbidden => "Blocked pages can't be unlocked.".to_owned(),
    }
}

/// How the last session went.
fn last_session_summary(ended: &EndedSession) -> String {
    let how = match ended.reason() {
        EndReason::Completed => "completed".to_owned(),
        EndReason::EndedEarly { reason: None } => "ended early".to_owned(),
        EndReason::EndedEarly {
            reason: Some(reason),
        } => format!("ended early (\"{reason}\")"),
        EndReason::Stopped { reason: None } => "stopped with the kill switch".to_owned(),
        EndReason::Stopped {
            reason: Some(reason),
        } => format!("stopped with the kill switch (\"{reason}\")"),
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
pub(crate) fn format_duration(duration: Duration) -> String {
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

    pub(crate) fn status_at(now: u64) -> StatusReport {
        StatusReport {
            now: Timestamp::from_millis(now),
            session: SessionState::Idle,
            pending_changes: Vec::new(),
            settings_problem: None,
            browsers: Vec::new(),
            foreground_app: None,
            start_at_login: true,
            start_at_login_off_after: None,
            start_at_login_problem: None,
            plan: ipc_proto::PlanStatus::default(),
            stop_allowed_at: None,
            stop_reason_required: false,
            friction: focus_core::Friction::default(),
            friction_loosen_after: None,
            unlocks_left: 3,
        }
    }

    fn beat(browser: &str, at: u64, incognito_allowed: bool) -> ipc_proto::BrowserStatus {
        ipc_proto::BrowserStatus {
            browser: browser.into(),
            last_seen: Timestamp::from_millis(at),
            incognito_allowed,
        }
    }

    #[test]
    fn warns_about_unprotected_browsers() {
        let mut status = status_at(100_000);
        assert_eq!(
            browser_lines(&status)[0].tone,
            Tone::Bad,
            "no extension at all"
        );

        status.browsers = vec![
            beat("chrome", 99_000, true),
            beat("edge", 99_000, false),
            beat("brave", 1_000, true),
        ];
        let lines = browser_lines(&status);
        assert_eq!(
            lines[0],
            Line {
                text: "Chrome: protected.".into(),
                tone: Tone::Good
            }
        );
        assert!(lines[1].tone == Tone::Bad && lines[1].text.contains("Allow in Incognito"));
        assert!(lines[2].tone == Tone::Bad && lines[2].text.contains("stopped checking in"));
    }

    #[test]
    fn describes_start_at_login() {
        let mut status = status_at(0);
        assert_eq!(login_line(&status).text, "Starts at login.");
        status.start_at_login_off_after = Some(Timestamp::from_millis(90_000));
        assert!(login_line(&status).text.contains("in 1:30"));
        status.start_at_login = false;
        assert!(
            login_line(&status)
                .text
                .starts_with("Doesn't start at login")
        );
        status.start_at_login_problem = Some("not available on Linux".into());
        assert!(
            login_line(&status)
                .text
                .contains("isn't set up: not available on Linux")
        );
    }

    #[test]
    fn summarises_pending_changes() {
        let mut status = status_at(0);
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

    #[test]
    fn labels_when_a_session_starts() {
        let now = Timestamp::from_millis(1_800_000_000_000);
        assert_eq!(when_label(now, now), "Now");
        let soon = when_label(now.saturating_add(Duration::from_secs(60)), now);
        assert!(soon.contains(':'), "{soon}");
    }
}
