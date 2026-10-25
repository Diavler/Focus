//! The Settings overlay, opened from the sidebar's foot: automatic starts, configurations, shared lists, and
//! general settings with the kill switch. Edits are kept in a draft until Save sends them to the
//! daemon, which writes `settings.toml`.

use std::time::{Duration, Instant};

use focus_core::{AutoStartRule, EditableSettings};
use iced::widget::{Column, button, column, pick_list, row, space, text, text_input, toggler};
use iced::{Alignment, Element, Length};
use ipc_proto::StatusReport;

use crate::configs;
use crate::help::info;
use crate::rules;
use crate::sessions::{format_duration, login_line};
use crate::style::{self, Tone, caption, card, dot, heading, title, toned};
use crate::{App, Message};

/// The tabs of the Rules page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// Block configurations.
    #[default]
    Configs,
    /// Shared lists.
    Lists,
    /// Automatic-start rules.
    AutoStarts,
    /// The friction limits.
    Limits,
}

/// How long after the last edit the settings save themselves, so typing isn't saved on every
/// key.
pub const AUTOSAVE_AFTER: Duration = Duration::from_secs(1);

/// The settings being edited.
#[derive(Debug, Clone)]
pub struct Draft {
    /// As last read from the daemon.
    saved: EditableSettings,
    /// As edited (automatic starts live in `rules`).
    pub edited: EditableSettings,
    /// The automatic-start rules as edited.
    pub rules: Vec<rules::Draft>,
    /// The configurations tab.
    pub configs: configs::Editor,
    /// The lists tab.
    pub lists: configs::Editor,
    /// Why the last save failed, if it did.
    pub error: Option<String>,
    /// What the save on its way sent, until the settings are read back.
    pub sent: Option<EditableSettings>,
    /// When the last edit not yet saved was made.
    edited_at: Option<Instant>,
}

impl Draft {
    /// A draft of `settings`, unchanged.
    pub fn new(settings: EditableSettings) -> Self {
        Self {
            rules: settings
                .auto_start
                .iter()
                .cloned()
                .map(rules::Draft::from_rule)
                .collect(),
            edited: settings.clone(),
            saved: settings,
            configs: configs::Editor::default(),
            lists: configs::Editor::default(),
            error: None,
            sent: None,
            edited_at: None,
        }
    }

    /// Takes in `settings` just read from the daemon without losing edits. The editors keep their
    /// place.
    pub fn reload(&mut self, settings: EditableSettings) {
        let saved_by_us = self
            .sent
            .take()
            .is_some_and(|sent| same_content(&sent, &sent.auto_start, &settings));
        if !self.dirty() || same_content(&self.edited, self.edited_rules(), &settings) {
            // Nothing edited would be lost.
            let configs = std::mem::take(&mut self.configs);
            let lists = std::mem::take(&mut self.lists);
            *self = Self::new(settings);
            self.configs = configs;
            self.lists = lists;
        } else if saved_by_us {
            // Our save landed and more was edited meanwhile: keep those edits on top of it.
            self.edited.revision.clone_from(&settings.revision);
            self.saved = settings;
        }
        // Otherwise the file was changed elsewhere: the edits stay, and saving them is refused
        // with "Reload from file".
    }

    /// Notes an edit made at `now`, which restarts the wait before saving.
    pub fn touch(&mut self, now: Instant) {
        self.edited_at = Some(now);
    }

    /// Whether the edits should be saved at `now`: something changed, nothing has been edited for
    /// [`AUTOSAVE_AFTER`], and no save is on its way.
    pub fn autosave_due(&self, now: Instant) -> bool {
        self.sent.is_none()
            && self
                .edited_at
                .is_some_and(|at| now.saturating_duration_since(at) >= AUTOSAVE_AFTER)
            && self.dirty()
    }

    /// What saving sends; the edits count as saved from here on.
    pub fn take_save(&mut self) -> EditableSettings {
        self.edited_at = None;
        let mut settings = self.edited.clone();
        settings.auto_start = self.edited_rules().cloned().collect();
        settings
    }

    /// Whether anything was edited. Called on every redraw, so it compares in place.
    pub fn dirty(&self) -> bool {
        !same_content(&self.edited, self.edited_rules(), &self.saved)
    }

    fn edited_rules(&self) -> impl Iterator<Item = &AutoStartRule> {
        self.rules.iter().map(|draft| &draft.rule)
    }

    /// The configuration names as edited, for the rule editor.
    pub fn config_names(&self) -> Vec<String> {
        self.edited
            .configs
            .iter()
            .map(|config| config.name.clone())
            .collect()
    }
}

/// Whether `settings`, with `auto_start` as its automatic starts, says the same as `other`. The
/// revision is left out: it only names the text the settings were read from.
fn same_content<'a>(
    settings: &EditableSettings,
    auto_start: impl IntoIterator<Item = &'a AutoStartRule>,
    other: &EditableSettings,
) -> bool {
    // Destructured so a new field can't be forgotten here.
    let EditableSettings {
        start_at_login,
        lists,
        configs,
        auto_start: _,
        friction,
        parking_hotkey,
        theme,
        timer_on_top,
        revision: _,
    } = settings;
    *start_at_login == other.start_at_login
        && *lists == other.lists
        && *configs == other.configs
        && *friction == other.friction
        && *parking_hotkey == other.parking_hotkey
        && *theme == other.theme
        && *timer_on_top == other.timer_on_top
        && auto_start.into_iter().eq(&other.auto_start)
}

/// One thing done on the Settings page.
#[derive(Debug, Clone)]
pub enum Msg {
    Tab(Tab),
    Config(configs::Edit),
    List(configs::Edit),
    StartAtLogin(bool),
    Friction(crate::friction::Edit),
    ParkingHotkey(focus_core::Hotkey),
    Theme(focus_core::Theme),
    TimerOnTop(bool),
    Save,
    Reload,
}

impl App {
    /// The Settings page: how Focus itself behaves (start at login, theme, the timer card, the
    /// parking shortcut) and the kill switch.
    pub(crate) fn settings_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let mut header = row![
            title("Settings"),
            info("Changes save automatically."),
            space::horizontal(),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if let Some(draft) = &self.settings {
            header = header.push(self.save_status(draft));
        }
        header = header.push(self.close_panel_key());
        let page: Column<'a, Message> = column![header].spacing(18);
        let Some(draft) = &self.settings else {
            // The kill switch doesn't need the settings, so it's there even if they can't be read.
            return page
                .push(self.settings_unavailable())
                .push(self.kill_switch_card(status))
                .into();
        };
        page.push(self.general_tab(draft, status)).into()
    }

    /// Why there are no settings to edit: still loading, or the file can't be read.
    pub(crate) fn settings_unavailable<'a>(&'a self) -> Element<'a, Message> {
        let Some(problem) = &self.settings_problem else {
            return caption("Loading the settings…").into();
        };
        card(
            column![
                toned(
                    format!("Focus can't read settings.toml: {problem}"),
                    Tone::Bad
                )
                .size(14),
                caption("Fix the file by hand (Stop Focus below still works), then:"),
                button("Try again")
                    .style(style::key)
                    .on_press(Message::Settings(Msg::Reload)),
            ]
            .spacing(10),
        )
        .into()
    }

    /// Whether the edits are saved. Saving happens by itself shortly after an edit.
    pub(crate) fn save_status<'a>(&'a self, draft: &'a Draft) -> Element<'a, Message> {
        if let Some(error) = &draft.error {
            return row![
                toned(format!("Not saved: {error}"), Tone::Bad).size(13),
                button(text("Reload from file").size(13))
                    .style(style::key)
                    .on_press(Message::Settings(Msg::Reload)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into();
        }
        if rules::problem(&draft.rules, &self.rule_configs).is_some() {
            return toned("Not saved until the rule is fixed", Tone::Warn)
                .size(13)
                .into();
        }
        if draft.dirty() || draft.sent.is_some() {
            caption("Saving…").into()
        } else {
            caption("Saved").into()
        }
    }

    /// Start at login, theme, the parking shortcut and the kill switch.
    fn general_tab<'a>(
        &'a self,
        draft: &'a Draft,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        let login = login_line(status);
        let settings = card(
            column![
                column![
                    setting_row(
                        "Start at login",
                        "Brings Focus back after a restart. Turning it off waits 24 hours.",
                        toggler(draft.edited.start_at_login)
                            .size(22)
                            .style(style::switch)
                            .on_toggle(|on| Message::Settings(Msg::StartAtLogin(on))),
                    ),
                    row![dot(login.tone), toned(login.text, login.tone).size(13)]
                        .spacing(8)
                        .align_y(Alignment::Center),
                ]
                .spacing(6),
                setting_row(
                    "Theme",
                    "System follows Windows' light or dark mode.",
                    style::segmented(
                        &focus_core::Theme::ALL,
                        draft.edited.theme,
                        |theme| theme.to_string(),
                        |theme| Message::Settings(Msg::Theme(theme)),
                    ),
                ),
                setting_row(
                    "Keep the timer on top",
                    "During a session, the small timer card stays above other windows.",
                    toggler(draft.edited.timer_on_top)
                        .size(22)
                        .style(style::switch)
                        .on_toggle(|on| Message::Settings(Msg::TimerOnTop(on))),
                ),
                setting_row(
                    "Parking lot shortcut",
                    "Opens the parking lot from any app. Enter parks the thought and hides Focus.",
                    pick_list(
                        focus_core::Hotkey::ALL,
                        Some(draft.edited.parking_hotkey),
                        |hotkey| Message::Settings(Msg::ParkingHotkey(hotkey)),
                    )
                    .style(style::picker)
                    .menu_style(style::menu),
                ),
            ]
            .spacing(20),
        );
        column![settings, self.kill_switch_card(status)]
            .spacing(16)
            .into()
    }

    /// Stopping Focus entirely: ask, wait, then confirm. Kept apart from the settings, and
    /// quiet until it's asked for.
    fn kill_switch_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            heading("Stop Focus"),
            info(
                "Ends any session, lifts blocking and closes Focus. With no session it closes at \
                 once; during one it waits 1 minute first (5 and a reason when Locked). Focus \
                 starts again at login or when opened."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let body: Element<'a, Message> = match status.stop_allowed_at {
            None => row![
                caption(if status.session.running().is_some() {
                    "Stop Focus completely, for when something has gone wrong."
                } else {
                    "Close Focus until you next log in or open it."
                }),
                space::horizontal(),
                button(if status.session.running().is_some() {
                    "Stop Focus…"
                } else {
                    "Close Focus…"
                })
                .style(style::key)
                .on_press_maybe(self.idle_then(Message::RequestStop)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            Some(allowed_at) if status.now < allowed_at => row![
                style::readout(
                    style::mono(format_duration(
                        allowed_at.saturating_duration_since(status.now)
                    ))
                    .size(15)
                ),
                caption("until you can stop Focus. Take the moment: is this worth it?"),
                space::horizontal(),
                button("Cancel")
                    .style(style::key)
                    .on_press_maybe(self.idle_then(Message::CancelStop)),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            Some(_) => {
                let needs_reason = status.stop_reason_required;
                let ready = !needs_reason || !self.stop_reason.trim().is_empty();
                let confirm = ready
                    .then_some(Message::ConfirmStop)
                    .and_then(|message| self.idle_then(message));
                let what = match status.session.running() {
                    Some(running) => format!(
                        "This ends the {} session now ({} left), lifts all blocking and closes \
                         Focus.",
                        running.spec().config.name,
                        format_duration(running.time_left(status.now))
                    ),
                    None => match self.plan.as_ref().and_then(|plan| plan.upcoming.first()) {
                        Some(next) => format!(
                            "This closes Focus. Nothing is blocked until it runs again, and {} \
                             ({}) won't start by itself.",
                            next.title,
                            crate::sessions::when_label(next.starts_at, status.now)
                        ),
                        None => {
                            "This closes Focus. Nothing is blocked until it runs again.".to_owned()
                        }
                    },
                };
                let mut confirm_column = column![
                    toned("Are you sure?", Tone::Bad).size(17),
                    text(what).size(14),
                ]
                .spacing(10);
                if needs_reason {
                    confirm_column = confirm_column.push(
                        text_input(
                            "Why do you need to stop a locked session?",
                            &self.stop_reason,
                        )
                        .style(style::field)
                        .on_input(Message::StopReasonChanged)
                        .on_submit_maybe(confirm.clone()),
                    );
                }
                confirm_column
                    .push(
                        row![
                            button("Stop Focus now")
                                .style(style::key_danger)
                                .on_press_maybe(confirm),
                            button("Keep Focus running")
                                .style(style::key)
                                .on_press_maybe(self.idle_then(Message::CancelStop)),
                        ]
                        .spacing(10),
                    )
                    .into()
            }
        };
        card(column![header, body].spacing(12).width(Length::Fill)).into()
    }
}

/// One setting on one line: its name and help on the left, its control on the right.
fn setting_row<'a>(
    name: &'a str,
    help: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        style::field_label(name),
        info(help),
        space::horizontal(),
        control.into()
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use focus_core::Strictness;

    use super::*;

    fn settings(revision: &str) -> EditableSettings {
        EditableSettings {
            start_at_login: true,
            lists: Vec::new(),
            configs: Vec::new(),
            auto_start: Vec::new(),
            friction: focus_core::Friction::default(),
            parking_hotkey: focus_core::Hotkey::default(),
            theme: focus_core::Theme::default(),
            timer_on_top: true,
            revision: revision.into(),
        }
    }

    fn rule(config: &str) -> AutoStartRule {
        AutoStartRule {
            config: config.into(),
            strictness: Strictness::Deep,
            calendar_keywords: vec!["#deep".into()],
            task_labels: Vec::new(),
            task_duration: Duration::from_secs(3000),
        }
    }

    #[test]
    fn saves_itself_after_a_pause_in_editing() {
        let start = Instant::now();
        let mut draft = Draft::new(settings("1"));
        draft.touch(start);
        assert!(
            !draft.autosave_due(start + AUTOSAVE_AFTER),
            "nothing changed"
        );
        draft.edited.start_at_login = false;
        draft.touch(start);
        assert!(!draft.autosave_due(start), "still editing");
        assert!(draft.autosave_due(start + AUTOSAVE_AFTER));
        let sent = draft.take_save();
        draft.sent = Some(sent);
        assert!(
            !draft.autosave_due(start + AUTOSAVE_AFTER * 2),
            "once is enough"
        );
    }

    #[test]
    fn dirty_follows_edits_and_rules() {
        let mut draft = Draft::new(settings("1"));
        assert!(!draft.dirty());
        draft.edited.start_at_login = false;
        assert!(draft.dirty());
        let mut draft = Draft::new(settings("1"));
        draft.rules.push(rules::Draft::from_rule(rule("coding")));
        assert!(draft.dirty());
    }

    #[test]
    fn reopening_keeps_unsaved_edits() {
        let mut draft = Draft::new(settings("1"));
        draft.edited.start_at_login = false;
        draft.reload(settings("1"));
        assert!(!draft.edited.start_at_login, "the edit is still there");
        assert!(draft.dirty());
    }

    #[test]
    fn a_clean_draft_follows_the_file() {
        let mut draft = Draft::new(settings("1"));
        let mut changed = settings("2");
        changed.start_at_login = false;
        draft.reload(changed.clone());
        assert_eq!(draft.take_save(), changed);
        assert!(!draft.dirty());
    }

    #[test]
    fn edits_made_while_saving_stay_on_top_of_the_save() {
        let mut draft = Draft::new(settings("1"));
        draft.edited.start_at_login = false;
        draft.sent = Some(draft.take_save());
        // Typed while the save was on its way.
        draft.rules.push(rules::Draft::from_rule(rule("coding")));

        let mut stored = settings("2");
        stored.start_at_login = false;
        draft.reload(stored);
        let next = draft.take_save();
        assert_eq!(
            next.revision, "2",
            "the next save won't be refused as stale"
        );
        assert!(!next.start_at_login);
        assert_eq!(next.auto_start, [rule("coding")]);
        assert!(draft.dirty(), "the rule is still unsaved");
    }

    #[test]
    fn edits_survive_a_change_made_elsewhere() {
        let mut draft = Draft::new(settings("1"));
        draft.edited.start_at_login = false;
        let mut elsewhere = settings("2");
        elsewhere.lists.push(focus_core::ListEntry {
            name: "social".into(),
            patterns: focus_core::RulePatterns::default(),
        });
        draft.reload(elsewhere);
        assert_eq!(
            draft.take_save().revision,
            "1",
            "so saving is refused as stale"
        );
        assert!(!draft.edited.start_at_login);
    }
}
