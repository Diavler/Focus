//! The Sessions page: the running session (or how the last one went), what starts next, starting a
//! session by hand, and whether browsers and login are protected.

use std::time::Duration;

use chrono::{Local, TimeZone};
use focus_core::{
    BlockConfig, BrowserPolicy, EndEarlyPolicy, EndReason, EndedSession, OverridePolicy,
    RunningSession, Strictness, Timestamp, ports::Silencing,
};
use iced::widget::{button, column, container, pick_list, row, slider, space, text, text_input};
use iced::{Alignment, Element, Length};
use ipc_proto::StatusReport;

use crate::agenda::clock;
use crate::help::info;
use crate::plan_pages::Line;
use crate::style::{self, Tone, bold, caption, card, heading, hero, title, toned};
use crate::{App, MINUTES, Message};

impl App {
    /// The Sessions page.
    pub(crate) fn sessions_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            title("Sessions"),
            info("Sessions run in the background; closing this window doesn't end them."),
        ]
        .spacing(12)
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
        // The session on the left, the parking lot beside it, as on the panel's face.
        let main: Element<'a, Message> = match status.session.running() {
            Some(running) => self.running_card(running, status),
            None => self.new_session_card(status),
        };
        let mut side = column![self.parking_card(status)].spacing(18);
        if let Some(pending) = pending_summary(status) {
            side = side.push(card(
                column![heading("Cooling off"), caption(pending)].spacing(8),
            ));
        }
        page.push(
            row![
                container(main).width(Length::FillPortion(3)),
                side.width(Length::FillPortion(2)),
            ]
            .spacing(18),
        )
        .into()
    }

    fn running_card<'a>(
        &'a self,
        running: &'a RunningSession,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        let spec = running.spec();
        let left = running.time_left(status.now);
        let total = running
            .ends_at()
            .saturating_duration_since(running.started_at());
        // Big digits over a bar of what's left, both behind the readout's glass.
        let time = style::readout(
            column![
                row![
                    style::lcd(format_duration(left)).size(54),
                    space::horizontal(),
                    style::label(&format!("of {}", format_duration(total))),
                ]
                .align_y(Alignment::Center),
                style::meter(share(left, total)),
            ]
            .spacing(12),
        )
        .padding([18, 20])
        .width(Length::Fill);
        hero(
            column![
                row![
                    column![
                        style::label("Running"),
                        bold(spec.config.name.as_str()).size(24)
                    ]
                    .spacing(2),
                    space::horizontal(),
                    style::tag(&spec.strictness.to_string()),
                ]
                .align_y(Alignment::Center),
                time,
                caption(format!(
                    "Ends at {}. {}",
                    clock(running.ends_at(), &Local),
                    unlock_line(running, status)
                )),
                self.do_not_disturb(status),
                self.end_controls(running, status),
            ]
            .spacing(16),
        )
        .into()
    }

    /// A button to turn on Do Not Disturb, where Focus can't do it itself (Windows).
    fn do_not_disturb<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        if status.notifications != Some(Silencing::AskUser) {
            return space::vertical().height(0).into();
        }
        row![
            button(text("Turn on Do Not Disturb").size(14))
                .style(style::key)
                .on_press_maybe(self.idle_then(Message::OpenNotificationSettings)),
            info(
                "Windows won't let apps switch this on, so this opens the setting. Turn it off yourself afterwards."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    /// The next session the plan will start, as a readout for the sidebar; `None` when nothing
    /// is planned.
    pub(crate) fn next_start_readout<'a>(
        &'a self,
        status: &'a StatusReport,
    ) -> Option<Element<'a, Message>> {
        let start = self.plan.as_ref()?.upcoming.first()?;
        Some(
            column![
                style::label("Next automatic start"),
                style::readout(
                    column![
                        row![
                            style::mono(when_label(start.starts_at, status.now)).size(13),
                            space::horizontal(),
                            style::mono(start.strictness.to_string().to_uppercase()).size(13),
                        ]
                        .spacing(6),
                        style::mono(start.title.as_str()).size(12),
                    ]
                    .spacing(4),
                )
                .width(Length::Fill),
            ]
            .spacing(6)
            .into(),
        )
    }

    /// Starting a session by hand, under how the last one went (if there was one).
    fn new_session_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let mut config = row![
            pick_list(
                self.config_names.as_slice(),
                self.selected.as_ref(),
                Message::ConfigPicked
            )
            .placeholder("no configurations")
            .style(style::picker)
            .menu_style(style::menu),
            info("Which sites and apps are restricted. Edit in Rules → Configurations."),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if let Some(summary) = self.selected_config().map(config_summary) {
            config = config.push(caption(summary));
        }
        // The length on the readout's glass, set with the fader under it.
        let duration = column![
            style::readout(
                row![
                    style::lcd_input(&self.minutes_text)
                        .on_input(Message::MinutesTyped)
                        .on_submit(Message::MinutesSubmitted),
                    space::horizontal(),
                    style::label("Duration"),
                    info("Ends by itself when the time is up. Type minutes (45), m:ss or h:mm:ss, or drag \
                         the slider."),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([18, 20])
            .width(Length::Fill),
            slider(MINUTES, self.minutes, Message::MinutesChanged).style(style::fader),
        ]
        .spacing(10);
        let can_start = self.selected.is_some() && !self.action_pending;
        let controls = row![
            style::segmented(
                &Strictness::ALL,
                self.strictness,
                |level| level.to_string(),
                Message::StrictnessPicked,
            ),
            info(strictness_help()),
            space::horizontal(),
            button(text("Start session").size(15))
                .padding([10, 20])
                .style(style::key_accent)
                .on_press_maybe(can_start.then_some(Message::Start)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut body = column![heading("Start a session")].spacing(16);
        if let Some(ended) = status.session.ended() {
            body = body.push(caption(last_session_summary(ended)));
        }
        card(body.push(config).push(duration).push(controls)).into()
    }

    /// End-early controls, which depend on the session's strictness.
    fn end_controls<'a>(
        &'a self,
        running: &'a RunningSession,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        match running.spec().strictness.end_early_policy() {
            EndEarlyPolicy::Immediate => button("End session")
                .style(style::key_danger)
                .on_press_maybe(self.idle_then(Message::EndEarly))
                .into(),
            EndEarlyPolicy::Forbidden => {
                caption("Locked: this session can't be ended early.").into()
            }
            EndEarlyPolicy::WaitAndReason { .. } => match running.end_allowed_at() {
                None => button("Ask to end early")
                    .style(style::key)
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
                .style(style::field)
                .on_input(Message::ReasonChanged)
                .on_submit_maybe(confirm.clone()),
            row![
                button("End session")
                    .style(style::key_danger)
                    .on_press_maybe(confirm),
                button("Keep focusing")
                    .style(style::key_accent)
                    .on_press_maybe(self.idle_then(Message::CancelEnd)),
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

/// One short line per browser whose extension is checking in, or one saying none is. A browser
/// whose extension can't run in private windows still counts, with a warning light: it's closed
/// during sessions until that's allowed.
pub(crate) fn browser_lines(status: &StatusReport) -> Vec<Line> {
    let lines: Vec<Line> = status
        .browsers
        .iter()
        // The same limit the daemon enforces.
        .filter(|browser| {
            status.now.saturating_duration_since(browser.last_seen)
                <= BrowserPolicy::default().heartbeat_timeout
        })
        .map(|browser| Line {
            text: format!("{} connected", browser_name(&browser.browser)),
            tone: if browser.incognito_allowed {
                Tone::Good
            } else {
                Tone::Warn
            },
        })
        .collect();
    if lines.is_empty() {
        vec![Line {
            text: "No browser connected".to_owned(),
            tone: Tone::Bad,
        }]
    } else {
        lines
    }
}

/// A browser's heartbeat name as people write it.
fn browser_name(heartbeat: &str) -> String {
    match heartbeat {
        "chrome" => "Chrome".to_owned(),
        "edge" => "Edge".to_owned(),
        "brave" => "Brave".to_owned(),
        "opera" => "Opera".to_owned(),
        "chromium" => "Chromium".to_owned(),
        "firefox" => "Firefox".to_owned(),
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
            text: "Doesn't start at login.".to_owned(),
            tone: Tone::Warn,
        },
    }
}

/// How much of `total` is `part`, from 0 to 1 (0 when `total` is zero).
fn share(part: Duration, total: Duration) -> f32 {
    if total.is_zero() {
        return 0.0;
    }
    (part.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
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

/// How hard each strictness is to end early, for its tooltip.
fn strictness_help() -> String {
    Strictness::ALL
        .iter()
        .map(|&level| format!("{level}: {}.", describe(level)))
        .collect::<Vec<_>>()
        .join("\n")
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

/// `minutes` as the readout shows it, e.g. `25:00` or `1:30:00`.
pub(crate) fn minutes_label(minutes: u16) -> String {
    format_duration(Duration::from_secs(u64::from(minutes) * 60))
}

/// A session length typed on the readout: plain minutes (`45`), `m:ss` as it's shown (`25:00`)
/// or `h:mm:ss` (`1:30:00`). Seconds are dropped; the result is kept within [`MINUTES`]. `None`
/// if it isn't a length.
pub(crate) fn parse_minutes(text: &str) -> Option<u16> {
    let parts: Vec<u32> = text
        .trim()
        .split(':')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    let minutes = match parts.as_slice() {
        [minutes] | [minutes, _] => *minutes,
        [hours, minutes, _] => hours.checked_mul(60)?.checked_add(*minutes)?,
        _ => return None,
    };
    let minutes = u16::try_from(minutes).unwrap_or(u16::MAX);
    Some(minutes.clamp(*MINUTES.start(), *MINUTES.end()))
}

#[cfg(test)]
mod tests {
    use focus_core::{SessionState, Timestamp};
    use ipc_proto::PendingSummary;

    use super::*;

    #[test]
    fn shares_stay_between_zero_and_one() {
        let minute = Duration::from_secs(60);
        assert_eq!(share(minute, minute * 4), 0.25);
        assert_eq!(share(minute * 5, minute), 1.0);
        assert_eq!(share(minute, Duration::ZERO), 0.0);
    }

    #[test]
    fn reads_typed_lengths() {
        assert_eq!(parse_minutes("45"), Some(45));
        assert_eq!(parse_minutes(" 25:00 "), Some(25));
        assert_eq!(parse_minutes("1:30:00"), Some(90));
        assert_eq!(parse_minutes("0"), Some(1), "at least a minute");
        assert_eq!(parse_minutes("999"), Some(180), "at most three hours");
        assert_eq!(parse_minutes(""), None);
        assert_eq!(parse_minutes("abc"), None);
        assert_eq!(parse_minutes("1:2:3:4"), None);
        for minutes in [1, 25, 59, 60, 90, 180] {
            assert_eq!(parse_minutes(&minutes_label(minutes)), Some(minutes));
        }
    }

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
            parking_lot: ipc_proto::ParkingStatus::default(),
            notifications: None,
            parking_hotkey: focus_core::Hotkey::default(),
            theme: focus_core::Theme::default(),
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
    fn names_the_connected_browsers() {
        let mut status = status_at(100_000);
        assert_eq!(
            browser_lines(&status),
            vec![Line {
                text: "No browser connected".into(),
                tone: Tone::Bad
            }]
        );

        status.browsers = vec![
            beat("chrome", 99_000, true),
            beat("edge", 99_000, false),
            beat("brave", 1_000, true),
        ];
        assert_eq!(
            browser_lines(&status),
            vec![
                Line {
                    text: "Chrome connected".into(),
                    tone: Tone::Good
                },
                Line {
                    text: "Edge connected".into(),
                    tone: Tone::Warn
                },
            ],
            "Brave stopped checking in"
        );

        status.browsers = vec![beat("brave", 1_000, true)];
        assert_eq!(browser_lines(&status)[0].text, "No browser connected");
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
