//! The Settings page, opened from Sessions: automatic starts, configurations, shared lists, and
//! general settings with the kill switch. Edits are kept in a draft until Save sends them to the
//! daemon, which writes `settings.toml`.

use focus_core::{AutoStartRule, EditableSettings};
use iced::widget::{Column, button, column, pick_list, row, space, text, text_input, toggler};
use iced::{Alignment, Element, Length};
use ipc_proto::StatusReport;

use crate::configs::{self, Target};
use crate::help::info;
use crate::rules;
use crate::sessions::{format_duration, login_line, pending_summary};
use crate::style::{self, Tone, caption, card, dot, heading, title, toned};
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
    /// Start at login, friction limits and the kill switch.
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
    /// What the save on its way sent, until the settings are read back.
    pub sent: Option<EditableSettings>,
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

    /// Throws the edits away, back to what was last read.
    pub fn discard(&mut self) {
        *self = Self::new(self.saved.clone());
    }

    /// What Save sends.
    pub fn to_save(&self) -> EditableSettings {
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
        revision: _,
    } = settings;
    *start_at_login == other.start_at_login
        && *lists == other.lists
        && *configs == other.configs
        && *friction == other.friction
        && *parking_hotkey == other.parking_hotkey
        && *theme == other.theme
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
    Save,
    Discard,
    Reload,
}

impl App {
    /// The Settings page.
    pub(crate) fn settings_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let header = row![
            button(text("← Sessions").size(14))
                .style(style::key_flat)
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
        let tabs = style::segmented(
            &[Tab::Rules, Tab::Configs, Tab::Lists, Tab::General],
            self.settings_tab,
            |tab| {
                match tab {
                    Tab::Rules => "Automatic starts",
                    Tab::Configs => "Configurations",
                    Tab::Lists => "Lists",
                    Tab::General => "General",
                }
                .to_string()
            },
            |tab| Message::Settings(Msg::Tab(tab)),
        );
        let mut page: Column<'a, Message> = column![header, tabs].spacing(18);
        if let Some(pending) = pending_summary(status) {
            page = page.push(card(
                column![heading("Cooling off"), caption(pending)].spacing(8),
            ));
        }
        let Some(draft) = &self.settings else {
            page = page.push(self.settings_unavailable());
            // The kill switch doesn't need the settings, so it's there even if they can't be read.
            if self.settings_tab == Tab::General {
                page = page.push(self.kill_switch_card(status));
            }
            return page.into();
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

    /// Why there are no settings to edit: still loading, or the file can't be read.
    fn settings_unavailable<'a>(&'a self) -> Element<'a, Message> {
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
                caption("Fix the file by hand (Stop Focus under General still works), then:"),
                button("Try again")
                    .style(style::key)
                    .on_press(Message::Settings(Msg::Reload)),
            ]
            .spacing(10),
        )
        .into()
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
                    .style(style::key)
                    .on_press(Message::Settings(Msg::Reload)),
            );
        }
        bar = bar
            .push(
                button("Discard")
                    .style(style::key)
                    .on_press_maybe(dirty.then_some(Message::Settings(Msg::Discard))),
            )
            .push(button("Save").style(style::key_accent).on_press_maybe(save));
        let mut column = column![bar].spacing(8);
        if let Some(error) = &draft.error {
            column = column.push(toned(error.as_str(), Tone::Bad).size(14));
        }
        card(column).into()
    }

    /// Start at login, friction limits and the kill switch.
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
                    .size(22)
                    .style(style::switch)
                    .on_toggle(|on| Message::Settings(Msg::StartAtLogin(on))),
                row![dot(login.tone), toned(login.text, login.tone).size(14)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            ]
            .spacing(12),
        );
        let friction = crate::friction::view(
            &draft.edited.friction,
            status.friction_loosen_after.map(|at| (at, status.now)),
        );
        let shortcut = card(
            column![
                row![
                    heading("Parking lot shortcut"),
                    info(
                        "Pressed in any app, it brings up Focus with the parking lot ready to \
                         type into; Enter parks the thought and puts Focus away again. Pick \
                         another if an app you use already has this one."
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                pick_list(
                    focus_core::Hotkey::ALL,
                    Some(draft.edited.parking_hotkey),
                    |hotkey| Message::Settings(Msg::ParkingHotkey(hotkey)),
                )
                .style(style::picker)
                .menu_style(style::menu),
            ]
            .spacing(12),
        );
        let theme = card(
            column![
                row![
                    heading("Theme"),
                    info(
                        "System follows Windows' light or dark mode as it changes; Light and \
                         Dark keep one look whatever the system says. Applies once saved."
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                style::segmented(
                    &focus_core::Theme::ALL,
                    draft.edited.theme,
                    |theme| theme.to_string(),
                    |theme| Message::Settings(Msg::Theme(theme)),
                ),
            ]
            .spacing(12),
        );
        column![
            start_at_login,
            theme,
            shortcut,
            friction,
            self.kill_switch_card(status)
        ]
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
                    .style(style::key_danger)
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
                    .style(style::key)
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
    fn dirty_follows_edits_and_rules() {
        let mut draft = Draft::new(settings("1"));
        assert!(!draft.dirty());
        draft.edited.start_at_login = false;
        assert!(draft.dirty());
        draft.discard();
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
        assert_eq!(draft.to_save(), changed);
        assert!(!draft.dirty());
    }

    #[test]
    fn edits_made_while_saving_stay_on_top_of_the_save() {
        let mut draft = Draft::new(settings("1"));
        draft.edited.start_at_login = false;
        draft.sent = Some(draft.to_save());
        // Typed while the save was on its way.
        draft.rules.push(rules::Draft::from_rule(rule("coding")));

        let mut stored = settings("2");
        stored.start_at_login = false;
        draft.reload(stored);
        let next = draft.to_save();
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
            draft.to_save().revision,
            "1",
            "so saving is refused as stale"
        );
        assert!(!draft.edited.start_at_login);
    }
}
