//! The Settings page, opened from Sessions: automatic starts, configurations, shared lists, and
//! general settings with the kill switch. Edits are kept in a draft until Save sends them to the
//! daemon, which writes `settings.toml`.

use focus_core::EditableSettings;
use iced::widget::{Column, button, column, row, space, text, text_input, toggler};
use iced::{Alignment, Element, Length};
use ipc_proto::StatusReport;

use crate::configs::{self, Target};
use crate::help::info;
use crate::rules;
use crate::sessions::{format_duration, login_line, pending_summary};
use crate::style::{Tone, caption, card, dot, heading, tab, title, toned};
use crate::{App, Message, Page};

/// The tabs of the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// Automatic-start rules.
    #[default]
    Rules,
    /// Block configurations.
    Configs,
    /// Shared lists.
    Lists,
    /// Start at login and the kill switch.
    General,
}

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
        }
    }

    /// Throws the edits away, back to what was last read.
    pub fn discard(&mut self) {
        *self = Self::new(self.saved.clone());
    }

    /// What Save sends.
    pub fn to_save(&self) -> EditableSettings {
        let mut settings = self.edited.clone();
        settings.auto_start = self.rules.iter().map(|draft| draft.rule.clone()).collect();
        settings
    }

    /// Whether anything was edited.
    pub fn dirty(&self) -> bool {
        self.to_save() != self.saved
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

/// One thing done on the Settings page.
#[derive(Debug, Clone)]
pub enum Msg {
    Tab(Tab),
    Config(configs::Edit),
    List(configs::Edit),
    StartAtLogin(bool),
    Save,
    Discard,
    Reload,
}

impl App {
    /// The Settings page.
    pub(crate) fn settings_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            button(text("← Sessions").size(14))
                .style(button::text)
                .on_press(Message::Navigate(Page::Sessions)),
            title("Settings"),
            info(
                "Everything here is saved into settings.toml in Focus's data folder, which you \
                 can still edit by hand. Edits to a configuration that's running, or that a \
                 session in the next 24 hours will use, wait 24 hours before they apply \
                 (cooling off), so rules can't be loosened on impulse."
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let tabs = row![
            tab(
                "Automatic starts",
                self.settings_tab == Tab::Rules,
                Message::Settings(Msg::Tab(Tab::Rules))
            ),
            tab(
                "Configurations",
                self.settings_tab == Tab::Configs,
                Message::Settings(Msg::Tab(Tab::Configs))
            ),
            tab(
                "Lists",
                self.settings_tab == Tab::Lists,
                Message::Settings(Msg::Tab(Tab::Lists))
            ),
            tab(
                "General",
                self.settings_tab == Tab::General,
                Message::Settings(Msg::Tab(Tab::General))
            ),
        ]
        .spacing(8);
        let mut page: Column<'a, Message> = column![header, tabs].spacing(18);
        if let Some(pending) = pending_summary(status) {
            page = page.push(card(
                column![heading("Cooling off"), caption(pending)].spacing(8),
            ));
        }
        let Some(draft) = &self.settings else {
            return page.push(caption("Loading the settings…")).into();
        };
        let body: Element<'a, Message> = match self.settings_tab {
            Tab::Rules => rules::view(&draft.rules, &self.rule_configs),
            Tab::Configs => {
                configs::view(&draft.edited, &draft.configs, Target::Configs, &self.apps)
            }
            Tab::Lists => configs::view(&draft.edited, &draft.lists, Target::Lists, &self.apps),
            Tab::General => self.general_tab(draft, status),
        };
        page.push(body).push(self.save_bar(draft)).into()
    }

    /// Save, Discard and the result of the last save.
    fn save_bar<'a>(&'a self, draft: &'a Draft) -> Element<'a, Message> {
        let dirty = draft.dirty();
        let problem = rules::problem(&draft.rules, &self.rule_configs);
        let save = (dirty && problem.is_none())
            .then_some(Message::Settings(Msg::Save))
            .and_then(|message| self.idle_then(message));
        let mut bar = row![].spacing(10).align_y(Alignment::Center);
        bar = bar.push(if dirty {
            toned("Unsaved changes", Tone::Warn).size(14)
        } else {
            caption("Everything is saved to settings.toml.")
        });
        bar = bar.push(space::horizontal());
        if draft.error.is_some() {
            bar = bar.push(
                button("Reload from file")
                    .style(button::secondary)
                    .on_press(Message::Settings(Msg::Reload)),
            );
        }
        bar = bar
            .push(
                button("Discard")
                    .style(button::secondary)
                    .on_press_maybe(dirty.then_some(Message::Settings(Msg::Discard))),
            )
            .push(button("Save").style(button::success).on_press_maybe(save));
        let mut column = column![bar].spacing(8);
        if let Some(error) = &draft.error {
            column = column.push(toned(error.as_str(), Tone::Bad).size(14));
        }
        card(column).into()
    }

    /// Start at login and the kill switch.
    fn general_tab<'a>(
        &'a self,
        draft: &'a Draft,
        status: &'a StatusReport,
    ) -> Element<'a, Message> {
        let login = login_line(status);
        let start_at_login = card(
            column![
                row![
                    heading("Start at login"),
                    info(
                        "Starting at login is what brings Focus back after a restart, so \
                         turning it off waits 24 hours (cooling off) before it applies. \
                         Turning it on applies right away."
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                toggler(draft.edited.start_at_login)
                    .label("Start Focus when I log in")
                    .on_toggle(|on| Message::Settings(Msg::StartAtLogin(on))),
                row![dot(login.tone), toned(login.text, login.tone).size(14)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            ]
            .spacing(12),
        );
        column![start_at_login, self.kill_switch_card(status)]
            .spacing(16)
            .into()
    }

    /// Stopping Focus entirely: ask, wait, then confirm.
    fn kill_switch_card<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            heading("Stop Focus"),
            info(
                "An emergency exit: ends the running session (even a Locked one), lifts all \
                 blocking and closes Focus. Because it's a way out, it makes you wait first: \
                 1 minute, or 5 minutes and a typed reason during a Locked session. Focus \
                 starts again the next time you log in, or when you open it."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let body: Element<'a, Message> = match status.stop_allowed_at {
            None => column![
                caption(
                    "Ends any session and closes Focus until you open it again or log in again."
                ),
                button("Stop Focus…")
                    .style(button::danger)
                    .on_press_maybe(self.idle_then(Message::RequestStop)),
            ]
            .spacing(10)
            .into(),
            Some(allowed_at) if status.now < allowed_at => column![
                toned(
                    format!(
                        "You can stop Focus in {}. Take the moment: is this really worth it?",
                        format_duration(allowed_at.saturating_duration_since(status.now))
                    ),
                    Tone::Warn
                )
                .size(15),
                button("Cancel")
                    .style(button::secondary)
                    .on_press_maybe(self.idle_then(Message::CancelStop)),
            ]
            .spacing(10)
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
                    None => "This closes Focus. Nothing is blocked until it runs again.".to_owned(),
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
                        .on_input(Message::StopReasonChanged)
                        .on_submit_maybe(confirm.clone()),
                    );
                }
                confirm_column
                    .push(
                        row![
                            button("Stop Focus now")
                                .style(button::danger)
                                .on_press_maybe(confirm),
                            button("Keep Focus running")
                                .style(button::secondary)
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
