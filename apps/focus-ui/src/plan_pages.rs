//! The Calendar and Todoist pages: the plan itself, and connecting each source.

use std::time::Duration;

use focus_core::Timestamp;
use focus_core::ports::SecretKey;
use iced::widget::{Column, button, column, row, space, text_input};
use iced::{Alignment, Element};
use ipc_proto::{SourceState, SourceStatus, StatusReport};

use crate::agenda::{self, Group};
use crate::help::info;
use crate::style::{self, Tone, caption, card, dot, heading, title, toned};
use crate::{App, Message};

/// How many days the Calendar page shows.
const CALENDAR_DAYS: u64 = 7;

impl App {
    /// Events for the coming week, day by day.
    pub(crate) fn calendar_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let source = &status.plan.calendar;
        let connection = self.connection_card(
            SecretKey::CalendarUrl,
            source,
            source_line("Calendar", "events", source, status.now),
            "Paste your calendar's secret iCal address",
            "In Google Calendar on the web: Settings → click your calendar under \"Settings for \
             my calendars\" → Integrate calendar → \"Secret address in iCal format\" → copy. \
             Anyone with this address can read your calendar, so Focus keeps it in Windows \
             Credential Manager, not in settings.toml.",
            &self.calendar_input,
        );
        let groups = self
            .plan
            .as_ref()
            .map(|plan| agenda::calendar_days(plan, &chrono::Local, CALENDAR_DAYS))
            .unwrap_or_default();
        // Today and tomorrow always show; later days only if something's on.
        let groups: Vec<Group> = groups
            .into_iter()
            .enumerate()
            .filter(|(index, group)| *index < 2 || !group.rows.is_empty())
            .map(|(_, group)| group)
            .collect();
        plan_page(
            "Calendar",
            source,
            "Your calendar for the coming week. Events marked \"Starts ...\" begin a session by \
             themselves; set that up in Rules → Automatic starts.",
            groups,
            "Nothing on the calendar.",
            connection,
        )
    }

    /// Open tasks: overdue, today, tomorrow and later this week.
    pub(crate) fn todoist_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let source = &status.plan.tasks;
        let connection = self.connection_card(
            SecretKey::TodoistToken,
            source,
            source_line("Todoist", "tasks", source, status.now),
            "Paste your Todoist API token",
            "In Todoist: Settings → Integrations → Developer → copy the API token. It lets Focus \
             read your tasks (never change them). It's kept in Windows Credential Manager.",
            &self.token_input,
        );
        let groups: Vec<Group> = self
            .plan
            .as_ref()
            .map(|plan| agenda::task_groups(plan, &chrono::Local))
            .unwrap_or_default()
            .into_iter()
            .filter(|group| group.title == "Today" || !group.rows.is_empty())
            .collect();
        plan_page(
            "Todoist",
            source,
            "Open tasks that are overdue or due this week. A task due at a time with a label \
             from an automatic start begins a session by itself.",
            groups,
            "Nothing due.",
            connection,
        )
    }

    /// A source's connection: one status line with Change and Disconnect once it's connected,
    /// or the box to paste its address or token into.
    fn connection_card<'a>(
        &'a self,
        key: SecretKey,
        source: &SourceStatus,
        line: Line,
        placeholder: &'a str,
        how_to: &'a str,
        value: &'a str,
    ) -> Element<'a, Message> {
        let connected = !matches!(source.state, SourceState::NotConfigured);
        let status = row![dot(line.tone), toned(line.text, line.tone).size(14)]
            .spacing(8)
            .align_y(Alignment::Center);
        if connected && self.changing_secret != Some(key) {
            let disconnect: Element<'a, Message> = if self.confirm_disconnect == Some(key) {
                row![
                    toned("Disconnect and forget what was fetched?", Tone::Bad).size(14),
                    button("Disconnect")
                        .style(style::key_danger)
                        .on_press_maybe(self.idle_then(Message::RemoveSecret(key))),
                    button("Keep")
                        .style(style::key)
                        .on_press(Message::AskDisconnect(None)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            } else {
                button("Disconnect")
                    .style(style::key)
                    .on_press(Message::AskDisconnect(Some(key)))
                    .into()
            };
            return card(
                row![
                    status,
                    space::horizontal(),
                    button("Change")
                        .style(style::key)
                        .on_press(Message::ChangeSecret(Some(key))),
                    disconnect,
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .into();
        }
        let save = (!value.trim().is_empty())
            .then_some(Message::SaveSecret(key))
            .and_then(|message| self.idle_then(message));
        let mut form = row![
            text_input(placeholder, value)
                .secure(true)
                .style(style::field)
                .on_input(move |value| Message::SecretInput(key, value))
                .on_submit_maybe(save.clone()),
            button("Save").style(style::key_accent).on_press_maybe(save),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if connected {
            form = form.push(
                button("Cancel")
                    .style(style::key)
                    .on_press(Message::ChangeSecret(None)),
            );
        }
        card(
            column![
                row![
                    heading(if connected {
                        "Change connection"
                    } else {
                        "Connect"
                    }),
                    info(how_to)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                status,
                form,
            ]
            .spacing(12),
        )
        .into()
    }
}

/// A plan page: title and status, the groups as cards, and the connection card (first if the
/// source isn't connected yet, so that's the obvious next step).
fn plan_page<'a>(
    name: &'a str,
    source: &'a SourceStatus,
    explanation: &'a str,
    groups: Vec<Group>,
    empty: &'a str,
    connection: Element<'a, Message>,
) -> Element<'a, Message> {
    let header = row![title(name), info(explanation)]
        .spacing(10)
        .align_y(Alignment::Center);
    let page: Column<'a, Message> = column![header].spacing(18);
    let connected = !matches!(source.state, SourceState::NotConfigured);
    if !connected {
        return page.push(connection).into();
    }
    // The days (or task groups) as sections of one card.
    let mut sections = column![].spacing(22);
    for group in groups {
        let mut heading_row = row![heading(group.title)]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(detail) = group.detail {
            heading_row = heading_row.push(caption(detail));
        }
        let body: Element<'a, Message> = if group.rows.is_empty() {
            caption(empty).into()
        } else {
            agenda::rows_view(group.rows).into()
        };
        sections = sections.push(column![heading_row, body].spacing(10));
    }
    page.push(card(sections)).push(connection).into()
}

/// One status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) text: String,
    pub(crate) tone: Tone,
}

/// How one plan source is doing, e.g. `Calendar: synced 2 min ago (12 events).`
pub(crate) fn source_line(name: &str, noun: &str, source: &SourceStatus, now: Timestamp) -> Line {
    let count = format!("{} {noun}", source.items);
    let ago = source
        .fetched_at
        .map(|at| format_ago(now.saturating_duration_since(at)));
    let (text, tone) = match (&source.state, ago) {
        (SourceState::Unknown, _) => (format!("{name}: checking..."), Tone::Quiet),
        (SourceState::NotConfigured, _) => (format!("{name}: not set up."), Tone::Quiet),
        (SourceState::Ok, ago) => (
            format!("{name}: synced {} ({count}).", ago.unwrap_or_default()),
            Tone::Good,
        ),
        (SourceState::Failing { problem, offline }, Some(ago)) => {
            let why = if *offline {
                "offline"
            } else {
                problem.as_str()
            };
            let tone = if *offline { Tone::Warn } else { Tone::Bad };
            (format!("{name}: {why}; using {count} from {ago}."), tone)
        }
        (SourceState::Failing { problem, .. }, None) => (format!("{name}: {problem}."), Tone::Bad),
    };
    Line { text, tone }
}

/// `just now`, `5 min ago`, `3 h ago` or `2 days ago`.
pub(crate) fn format_ago(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_plan_sources() {
        let now = Timestamp::from_millis(3 * 3_600_000);
        let line = |source: &SourceStatus| source_line("Calendar", "events", source, now);
        let mut source = SourceStatus::default();
        assert_eq!(line(&source).text, "Calendar: checking...");
        source.state = SourceState::NotConfigured;
        assert_eq!(line(&source).text, "Calendar: not set up.");

        source.state = SourceState::Ok;
        source.fetched_at = Some(Timestamp::from_millis(3 * 3_600_000 - 120_000));
        source.items = 12;
        assert_eq!(
            line(&source).text,
            "Calendar: synced 2 min ago (12 events)."
        );
        assert_eq!(line(&source).tone, Tone::Good);

        source.state = SourceState::Failing {
            problem: "couldn't reach the service: dns".into(),
            offline: true,
        };
        source.fetched_at = Some(Timestamp::from_millis(0));
        let offline = line(&source);
        assert_eq!(
            offline.text,
            "Calendar: offline; using 12 events from 3 h ago."
        );
        assert_eq!(
            offline.tone,
            Tone::Warn,
            "being offline isn't something to fix"
        );

        source.state = SourceState::Failing {
            problem: "Todoist refused the token".into(),
            offline: false,
        };
        source.fetched_at = None;
        let refused = source_line("Todoist", "tasks", &source, now);
        assert_eq!(refused.text, "Todoist: Todoist refused the token.");
        assert_eq!(refused.tone, Tone::Bad);
    }

    #[test]
    fn formats_how_long_ago() {
        assert_eq!(format_ago(Duration::from_secs(30)), "just now");
        assert_eq!(format_ago(Duration::from_secs(5 * 60)), "5 min ago");
        assert_eq!(format_ago(Duration::from_secs(3 * 3600 + 59)), "3 h ago");
        assert_eq!(format_ago(Duration::from_secs(50 * 3600)), "2 days ago");
    }
}
