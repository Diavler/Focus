// No console window on Windows: the UI is a windowed program.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Iced desktop UI. Talks only to `focus-daemon` over IPC, so closing the window never ends a session.
//!
//! The window has a sidebar with four pages (Overview, Sessions, Calendar, Todoist) and a
//! Settings page opened from Sessions. Each page lives in its own module as an `impl App` block; this file holds
//! the state, the messages and how they change the state.

mod agenda;
mod configs;
mod daemon;
mod friction;
mod help;
mod overview;
mod parking;
mod plan_pages;
mod rules;
mod sessions;
mod settings;
mod style;
mod timeline;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use focus_core::ports::SecretKey;
use focus_core::{BlockConfig, EditableSettings, ParkedItem, Strictness};
use focus_install::{InstanceLock, Paths};
use iced::widget::{Column, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Subscription, Task};
use ipc_proto::{PlanView, Request, SourceState, StatusReport};
use ipc_transport::Endpoint;

use crate::style::{Tone, caption, dot, nav};

fn main() -> iced::Result {
    let install = focus_install::install_folder().unwrap_or_default();
    let paths = Paths::portable(&install);
    // One UI per install. The daemon checks this lock to see whether a UI is up, and starts one
    // during a session if not (ADR 0008).
    let _ = std::fs::create_dir_all(&paths.data);
    let _ui_lock = match InstanceLock::acquire_patiently(&paths.ui_lock) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            // Already open (often minimised): ask that window to come forward, then leave.
            let _ = std::fs::write(&paths.ui_show, b"");
            return Ok(());
        }
        Err(error) => {
            eprintln!("couldn't take {}: {error}", paths.ui_lock.display());
            None
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let minimized = args.iter().any(|arg| arg == "--minimized");
    // Started by a dry-run daemon: restart it as a dry run too.
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let _ = std::fs::remove_file(&paths.ui_show);
    let mut app = iced::application(
        move || App::new(install.clone(), minimized, dry_run),
        App::update,
        App::view,
    );
    for font in style::FONTS {
        app = app.font(font);
    }
    app.default_font(style::SANS)
        .title("Focus")
        .subscription(App::subscription)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: iced::Size::new(1120.0, 780.0),
            min_size: Some(MIN_WINDOW),
            position: iced::window::Position::Centered,
            ..iced::window::Settings::default()
        })
        .run()
}

/// The smallest the window can be made. The width keeps the Sessions start card's strictness
/// keys and Start key on one line beside the parking lot (sidebar 210 + padding + a 3:2 split);
/// the height keeps the sidebar's readouts, pages and status lines clear of each other.
const MIN_WINDOW: iced::Size = iced::Size::new(1080.0, 640.0);

/// How often (in polls, about seconds) a shortcut another app held is tried again.
const HOTKEY_RETRY_POLLS: u32 = 30;

/// Shortest and longest session the duration slider offers, in minutes.
pub(crate) const MINUTES: std::ops::RangeInclusive<u16> = 1..=180;

/// How often the window asks the daemon for its status.
const POLL: Duration = Duration::from_secs(1);

/// Configurations are re-fetched every this many polls, to pick up settings edits.
const CONFIGS_EVERY_POLLS: u32 = 10;

/// The calendar and tasks are re-fetched every this many polls.
const PLAN_EVERY_POLLS: u32 = 5;

/// After this many failed status polls in a row, the UI starts the daemon (ADR 0008).
const RESTART_DAEMON_AFTER: u32 = 3;

/// The pages of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    /// Today's calendar and tasks; the window opens on it.
    Overview,
    Sessions,
    Calendar,
    Todoist,
    /// Opened from Sessions.
    Settings,
}

/// Everything the window shows.
pub(crate) struct App {
    /// The folder holding the executables, for starting the daemon.
    install: PathBuf,
    endpoint: Endpoint,
    /// Failed status polls in a row.
    failed_polls: u32,
    /// Created by a second launch to ask this window to come forward.
    show_request: PathBuf,
    /// Restart the daemon with `--dry-run`.
    dry_run: bool,
    /// The last status the daemon sent; `None` until it first answers.
    status: Option<StatusReport>,
    /// Why the daemon can't be reached, if it can't.
    unreachable: Option<String>,
    page: Page,
    configs: Vec<BlockConfig>,
    config_names: Vec<String>,
    selected: Option<String>,
    strictness: Strictness,
    minutes: u16,
    /// The session length as typed on the readout.
    minutes_text: String,
    reason: String,
    /// The calendar and tasks, from the daemon; `None` until it first answers.
    plan: Option<PlanView>,
    /// A calendar address being typed or pasted; sent to the daemon's keychain on Save.
    calendar_input: String,
    /// A Todoist token being typed or pasted.
    token_input: String,
    /// The settings being edited; loaded when Settings opens.
    settings: Option<settings::Draft>,
    /// Why the settings couldn't be read, if the last try failed.
    settings_problem: Option<String>,
    settings_tab: settings::Tab,
    /// Configuration names as edited, offered by the rule editor.
    rule_configs: Vec<String>,
    /// Apps with a window open, offered by the configuration editor.
    apps: Vec<String>,
    /// The reason typed to stop Focus during a Locked session.
    stop_reason: String,
    /// Everything parked, from the daemon.
    parked: Vec<ParkedItem>,
    /// A parking-lot request is on its way.
    parked_pending: bool,
    /// A thought being typed into the parking lot.
    park_input: iced::widget::text_editor::Content,
    /// The window came up for the parking-lot shortcut, so it goes back down after parking.
    capturing: bool,
    /// Why the parking-lot shortcut couldn't be registered, if it couldn't.
    hotkey_problem: Option<String>,
    /// Whether registering is worth retrying (another app held the shortcut).
    hotkey_retry: bool,
    /// Bumped to register the shortcut again.
    hotkey_attempt: u32,
    /// The parking lot's revision the list was last read at.
    parked_revision: Option<u64>,
    error: Option<String>,
    polls: u32,
    /// A status request is on its way, so polls don't pile up if the daemon is slow.
    status_pending: bool,
    /// A configs request is on its way.
    configs_pending: bool,
    /// A plan request is on its way.
    plan_pending: bool,
    /// An action (start, end, save, ...) is on its way; its buttons are disabled until it answers.
    action_pending: bool,
    /// What the action on its way does, so its answer can be handled.
    saving: Option<Saving>,
    /// The system's light or dark mode, last reported.
    system_mode: iced::theme::Mode,
    /// The mode `theme` was built for.
    theme_mode: iced::theme::Mode,
    /// Focus's colours for the theme setting and the system's mode.
    theme: iced::Theme,
}

/// An action whose answer needs more than showing an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Saving {
    Secret(SecretKey),
    Settings,
    Stop,
    Park,
}

/// User actions, timer ticks and daemon answers.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Poll,
    /// Boxed: a status is much larger than the other messages.
    StatusLoaded(Box<Result<StatusReport, daemon::StatusError>>),
    ConfigsLoaded(Result<Vec<BlockConfig>, String>),
    /// Boxed: the plan is much larger than the other messages.
    PlanLoaded(Box<Result<PlanView, String>>),
    Navigate(Page),
    ConfigPicked(String),
    StrictnessPicked(Strictness),
    MinutesChanged(u16),
    MinutesTyped(String),
    MinutesSubmitted,
    Start,
    EndEarly,
    ReasonChanged(String),
    ConfirmEnd,
    CancelEnd,
    ActionDone(Result<(), String>),
    SecretInput(SecretKey, String),
    SaveSecret(SecretKey),
    RemoveSecret(SecretKey),
    OpenSettings,
    /// Boxed like the other large answers.
    SettingsLoaded(Box<Result<EditableSettings, String>>),
    AppsLoaded(Result<Vec<String>, String>),
    Settings(settings::Msg),
    Rule(rules::Edit),
    RequestStop,
    StopReasonChanged(String),
    ConfirmStop,
    CancelStop,
    /// The system switched between light and dark mode (or said which at startup).
    ThemeMode(iced::theme::Mode),
    ParkInput(iced::widget::text_editor::Action),
    Park,
    Unpark(u64),
    ClearSentParked,
    ParkedLoaded(Result<Vec<ParkedItem>, String>),
    /// The parking-lot shortcut was pressed somewhere.
    HotkeyPressed,
    /// The shortcut was registered (`problem` is `None`) or why it couldn't be.
    HotkeyRegistered {
        problem: Option<String>,
        retry: bool,
    },
    /// The window lost focus (ends a capture the shortcut started).
    WindowUnfocused,
    OpenNotificationSettings,
}

impl App {
    fn new(install: PathBuf, minimized: bool, dry_run: bool) -> (Self, Task<Message>) {
        let mut app = Self {
            endpoint: Endpoint::for_install(&install),
            show_request: Paths::portable(&install).ui_show,
            install,
            failed_polls: 0,
            dry_run,
            status: None,
            unreachable: None,
            page: Page::Overview,
            configs: Vec::new(),
            config_names: Vec::new(),
            selected: None,
            strictness: Strictness::Deep,
            minutes: 25,
            minutes_text: sessions::minutes_label(25),
            reason: String::new(),
            plan: None,
            calendar_input: String::new(),
            token_input: String::new(),
            settings: None,
            settings_problem: None,
            settings_tab: settings::Tab::default(),
            rule_configs: Vec::new(),
            apps: Vec::new(),
            stop_reason: String::new(),
            parked: Vec::new(),
            parked_pending: false,
            park_input: iced::widget::text_editor::Content::new(),
            capturing: false,
            hotkey_problem: None,
            hotkey_retry: false,
            hotkey_attempt: 0,
            parked_revision: None,
            error: None,
            polls: 0,
            status_pending: false,
            configs_pending: false,
            plan_pending: false,
            action_pending: false,
            saving: None,
            system_mode: iced::theme::Mode::Dark,
            theme_mode: iced::theme::Mode::Dark,
            theme: style::theme(iced::theme::Mode::Dark),
        };
        let mut boot = vec![
            app.fetch_status(),
            app.fetch_configs(),
            app.fetch_plan(),
            iced::system::theme().map(Message::ThemeMode),
        ];
        if minimized {
            boot.push(iced::window::oldest().and_then(|id| iced::window::minimize(id, true)));
        }
        (app, Task::batch(boot))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Poll => return self.poll(),
            Message::StatusLoaded(result) => {
                self.status_pending = false;
                match *result {
                    Ok(status) => {
                        let revision = status.parking_lot.revision;
                        self.status = Some(status);
                        self.refresh_theme();
                        self.unreachable = None;
                        self.failed_polls = 0;
                        if self.parked_revision != Some(revision) && !self.parked_pending {
                            self.parked_revision = Some(revision);
                            return self.fetch_parked();
                        }
                    }
                    Err(problem) => {
                        self.unreachable = Some(problem.message);
                        if problem.not_running {
                            self.restart_daemon_if_gone();
                        } else {
                            self.failed_polls = 0;
                        }
                    }
                }
            }
            Message::ConfigsLoaded(result) => {
                self.configs_pending = false;
                match result {
                    Ok(configs) => self.set_configs(configs),
                    Err(problem) => self.unreachable = Some(problem),
                }
            }
            Message::PlanLoaded(result) => {
                self.plan_pending = false;
                if let Ok(plan) = *result {
                    self.plan = Some(plan);
                }
            }
            Message::Navigate(page) => {
                self.page = page;
                if page == Page::Settings {
                    return self.open_settings();
                }
            }
            Message::ConfigPicked(name) => self.selected = Some(name),
            Message::StrictnessPicked(strictness) => self.strictness = strictness,
            Message::MinutesChanged(minutes) => {
                self.minutes = minutes;
                self.minutes_text = sessions::minutes_label(minutes);
            }
            Message::MinutesTyped(text) => {
                if let Some(minutes) = sessions::parse_minutes(&text) {
                    self.minutes = minutes;
                }
                self.minutes_text = text;
            }
            Message::MinutesSubmitted => {
                self.minutes_text = sessions::minutes_label(self.minutes);
            }
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
            Message::ActionDone(result) => return self.action_done(result),
            Message::SecretInput(key, value) => *self.secret_input(key) = value,
            Message::SaveSecret(key) => {
                // Cleared once the daemon has it in the keychain.
                let value = self.secret_input(key).clone();
                return self.act_saving(
                    Saving::Secret(key),
                    Request::SetSecret {
                        key,
                        value: Some(value),
                    },
                );
            }
            Message::RemoveSecret(key) => {
                self.secret_input(key).clear();
                return self.act(Request::SetSecret { key, value: None });
            }
            Message::OpenSettings => {
                self.page = Page::Settings;
                return self.open_settings();
            }
            Message::SettingsLoaded(result) => match *result {
                Ok(settings) => {
                    self.settings_problem = None;
                    match &mut self.settings {
                        Some(draft) => draft.reload(settings),
                        None => self.settings = Some(settings::Draft::new(settings)),
                    }
                    self.refresh_rule_configs();
                }
                Err(problem) => self.settings_problem = Some(problem),
            },
            Message::AppsLoaded(result) => {
                if let Ok(apps) = result {
                    self.apps = apps;
                }
            }
            Message::Settings(message) => return self.settings_message(message),
            Message::Rule(edit) => {
                if let Some(draft) = &mut self.settings {
                    rules::apply(&mut draft.rules, edit, &self.rule_configs);
                    draft.touch(Instant::now());
                }
            }
            Message::RequestStop => {
                self.stop_reason.clear();
                return self.act(Request::RequestStop);
            }
            Message::StopReasonChanged(reason) => self.stop_reason = reason,
            Message::ConfirmStop => {
                let reason = self.stop_reason.clone();
                return self.act_saving(Saving::Stop, Request::ConfirmStop { reason });
            }
            Message::CancelStop => {
                self.stop_reason.clear();
                return self.act(Request::CancelStop);
            }
            Message::ThemeMode(mode) => {
                self.system_mode = mode;
                self.refresh_theme();
            }
            Message::ParkInput(action) => self.park_input.perform(action),
            Message::Park => {
                let text = self.park_input.text().trim_end().to_owned();
                return self.act_saving(Saving::Park, Request::Park { text, url: None });
            }
            Message::Unpark(id) => return self.act(Request::Unpark { id }),
            Message::ClearSentParked => return self.act(Request::ClearSentParked),
            Message::ParkedLoaded(result) => {
                self.parked_pending = false;
                if let Ok(items) = result {
                    self.parked = items;
                }
            }
            Message::HotkeyPressed => {
                self.page = Page::Sessions;
                self.capturing = true;
                return Task::batch([
                    iced::window::oldest().and_then(|id| {
                        Task::batch([
                            iced::window::minimize(id, false),
                            iced::window::gain_focus(id),
                        ])
                    }),
                    iced::widget::operation::focus(parking::INPUT),
                ]);
            }
            Message::HotkeyRegistered { problem, retry } => {
                self.hotkey_problem = problem;
                self.hotkey_retry = retry;
            }
            Message::WindowUnfocused => self.capturing = false,
            Message::OpenNotificationSettings => {
                return self.act(Request::OpenNotificationSettings);
            }
        }
        Task::none()
    }

    /// The once-a-second poll: status every time, configs and plan now and then.
    fn poll(&mut self) -> Task<Message> {
        self.polls = self.polls.wrapping_add(1);
        // Another app held the shortcut: try again now and then.
        if self.hotkey_retry && self.polls.is_multiple_of(HOTKEY_RETRY_POLLS) {
            self.hotkey_attempt = self.hotkey_attempt.wrapping_add(1);
        }
        let mut tasks = Vec::new();
        if std::fs::remove_file(&self.show_request).is_ok() {
            tasks.push(iced::window::oldest().and_then(|id| {
                Task::batch([
                    iced::window::minimize(id, false),
                    iced::window::gain_focus(id),
                ])
            }));
        }
        if !self.status_pending {
            tasks.push(self.fetch_status());
        }
        let configs_due = self.polls.is_multiple_of(CONFIGS_EVERY_POLLS) || self.configs.is_empty();
        if configs_due && !self.configs_pending {
            tasks.push(self.fetch_configs());
        }
        let plan_due = self.polls.is_multiple_of(PLAN_EVERY_POLLS) || self.plan.is_none();
        if plan_due && !self.plan_pending {
            tasks.push(self.fetch_plan());
        }
        // Edited settings save themselves once editing pauses (and the rules make sense).
        let autosave = self.settings.as_ref().is_some_and(|draft| {
            draft.autosave_due(Instant::now())
                && rules::problem(&draft.rules, &self.rule_configs).is_none()
        });
        if autosave && !self.action_pending {
            tasks.push(self.settings_message(settings::Msg::Save));
        }
        Task::batch(tasks)
    }

    /// An action's answer: errors are shown, and saves that worked clear what was typed.
    fn action_done(&mut self, result: Result<(), String>) -> Task<Message> {
        self.action_pending = false;
        let saving = self.saving.take();
        match (saving, &result) {
            (Some(Saving::Stop), Ok(())) => {
                // The daemon has ended the session and is exiting; the window goes too.
                return iced::exit();
            }
            (Some(Saving::Secret(key)), Ok(())) => self.secret_input(key).clear(),
            // A failed park leaves the window up, with the error, so the thought isn't lost.
            (Some(Saving::Park), Err(_)) => self.capturing = false,
            (Some(Saving::Park), Ok(())) => {
                self.park_input = iced::widget::text_editor::Content::new();
                let mut tasks = vec![self.fetch_parked(), self.fetch_status()];
                if std::mem::take(&mut self.capturing) {
                    // Back to what the shortcut interrupted.
                    tasks.push(
                        iced::window::oldest().and_then(|id| iced::window::minimize(id, true)),
                    );
                }
                return Task::batch(tasks);
            }
            (Some(Saving::Settings), Ok(())) => {
                return Task::batch([
                    self.fetch_settings(),
                    self.fetch_status(),
                    self.fetch_configs(),
                ]);
            }
            (Some(Saving::Settings), Err(problem)) => {
                if let Some(draft) = &mut self.settings {
                    draft.sent = None;
                    draft.error = Some(problem.clone());
                }
                return self.fetch_status();
            }
            _ => {}
        }
        self.error = result.err();
        Task::batch([self.fetch_status(), self.fetch_plan()])
    }

    fn settings_message(&mut self, message: settings::Msg) -> Task<Message> {
        // Tabs and reloading work without settings, so the kill switch stays reachable when
        // settings.toml can't be read.
        match message {
            settings::Msg::Tab(tab) => {
                self.settings_tab = tab;
                if let Some(draft) = &mut self.settings {
                    draft.error = None;
                }
                if tab == settings::Tab::Configs {
                    return self.fetch_apps();
                }
                return Task::none();
            }
            settings::Msg::Reload => {
                // Throws the edits away on purpose: the file wins.
                self.settings = None;
                return self.fetch_settings();
            }
            _ => {}
        }
        let Some(draft) = &mut self.settings else {
            return Task::none();
        };
        draft.error = None;
        if !matches!(message, settings::Msg::Save) {
            draft.touch(Instant::now());
        }
        match message {
            settings::Msg::Tab(_) | settings::Msg::Reload => {}
            settings::Msg::Config(edit) => {
                let removed = configs::apply(
                    &mut draft.edited,
                    &mut draft.configs,
                    configs::Target::Configs,
                    edit,
                );
                if let Some(configs::Removed::Config(name)) = removed {
                    // Rules can't start a configuration that's gone.
                    draft.rules.retain(|rule| rule.rule.config != name);
                }
                self.refresh_rule_configs();
            }
            settings::Msg::List(edit) => {
                configs::apply(
                    &mut draft.edited,
                    &mut draft.lists,
                    configs::Target::Lists,
                    edit,
                );
            }
            settings::Msg::StartAtLogin(on) => draft.edited.start_at_login = on,
            settings::Msg::Friction(edit) => friction::apply(&mut draft.edited.friction, edit),
            settings::Msg::ParkingHotkey(hotkey) => draft.edited.parking_hotkey = hotkey,
            settings::Msg::Theme(theme) => draft.edited.theme = theme,
            settings::Msg::Save => {
                let settings = draft.take_save();
                draft.sent = Some(settings.clone());
                return self.act_saving(Saving::Settings, Request::SaveSettings { settings });
            }
        }
        Task::none()
    }

    fn open_settings(&mut self) -> Task<Message> {
        let mut tasks = vec![self.fetch_settings()];
        if self.settings_tab == settings::Tab::Configs {
            tasks.push(self.fetch_apps());
        }
        Task::batch(tasks)
    }

    fn refresh_rule_configs(&mut self) {
        self.rule_configs = self
            .settings
            .as_ref()
            .map(settings::Draft::config_names)
            .unwrap_or_default();
    }

    /// Starts the daemon after [`RESTART_DAEMON_AFTER`] polls in a row found nothing listening. A
    /// daemon that is running after all exits by itself, so this is always safe.
    fn restart_daemon_if_gone(&mut self) {
        self.failed_polls += 1;
        if self.failed_polls < RESTART_DAEMON_AFTER {
            return;
        }
        self.failed_polls = 0;
        let args: &[&str] = if self.dry_run { &["--dry-run"] } else { &[] };
        if let Err(error) = focus_install::spawn_sibling(&self.install, "focus-daemon", args) {
            self.error = Some(format!("couldn't start the Focus daemon: {error}"));
        }
    }

    fn secret_input(&mut self, key: SecretKey) -> &mut String {
        match key {
            SecretKey::CalendarUrl => &mut self.calendar_input,
            SecretKey::TodoistToken => &mut self.token_input,
        }
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
        Task::perform(daemon::status(self.endpoint.clone()), |result| {
            Message::StatusLoaded(Box::new(result))
        })
    }

    fn fetch_plan(&mut self) -> Task<Message> {
        self.plan_pending = true;
        Task::perform(daemon::plan(self.endpoint.clone()), |result| {
            Message::PlanLoaded(Box::new(result))
        })
    }

    fn fetch_configs(&mut self) -> Task<Message> {
        self.configs_pending = true;
        Task::perform(
            daemon::configs(self.endpoint.clone()),
            Message::ConfigsLoaded,
        )
    }

    fn fetch_settings(&self) -> Task<Message> {
        Task::perform(daemon::settings(self.endpoint.clone()), |result| {
            Message::SettingsLoaded(Box::new(result))
        })
    }

    fn fetch_parked(&mut self) -> Task<Message> {
        self.parked_pending = true;
        Task::perform(
            daemon::parking_lot(self.endpoint.clone()),
            Message::ParkedLoaded,
        )
    }

    fn fetch_apps(&self) -> Task<Message> {
        Task::perform(daemon::apps(self.endpoint.clone()), Message::AppsLoaded)
    }

    fn act(&mut self, request: Request) -> Task<Message> {
        self.action_pending = true;
        Task::perform(
            daemon::act(self.endpoint.clone(), request),
            Message::ActionDone,
        )
    }

    fn act_saving(&mut self, saving: Saving, request: Request) -> Task<Message> {
        self.saving = Some(saving);
        self.act(request)
    }

    /// `message`, unless an action is still waiting for the daemon (so double clicks don't send
    /// it twice).
    pub(crate) fn idle_then(&self, message: Message) -> Option<Message> {
        (!self.action_pending).then_some(message)
    }

    /// Polls the daemon once a second, and follows the system's light or dark mode.
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(POLL).map(|_| Message::Poll),
            iced::system::theme_changes().map(Message::ThemeMode),
        ];
        // Registered once the daemon has said which shortcut the settings ask for.
        if let Some(status) = &self.status {
            subscriptions.push(parking::hotkey_subscription(
                status.parking_hotkey,
                self.hotkey_attempt,
            ));
        }
        if self.capturing {
            subscriptions.push(iced::window::events().filter_map(|(_, event)| {
                (event == iced::window::Event::Unfocused).then_some(Message::WindowUnfocused)
            }));
        }
        Subscription::batch(subscriptions)
    }

    /// Focus's colours (kept, since building a theme derives a whole palette).
    fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    /// Rebuilds the theme if the setting or the system's mode now asks for another one. Follows
    /// the system until the daemon has said what the setting is.
    fn refresh_theme(&mut self) {
        let chosen = self.status.as_ref().map(|status| status.theme);
        let mode = style::mode_for(chosen.unwrap_or_default(), self.system_mode);
        if mode != self.theme_mode {
            self.theme_mode = mode;
            self.theme = style::theme(mode);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match (&self.status, &self.unreachable) {
            (_, Some(problem)) => column![
                style::title("Can't reach the Focus daemon"),
                text("Starting it again; this window reconnects by itself."),
                caption(problem.as_str()),
            ]
            .spacing(10)
            .into(),
            (None, None) => style::title("Connecting to Focus…").into(),
            (Some(status), None) => match self.page {
                Page::Overview => self.overview_page(status),
                Page::Sessions => self.sessions_page(status),
                Page::Calendar => self.calendar_page(status),
                Page::Todoist => self.todoist_page(status),
                Page::Settings => self.settings_page(status),
            },
        };
        let mut main: Column<'_, Message> = column![content].spacing(18);
        if let Some(error) = &self.error {
            main = main.push(style::toned(error.as_str(), Tone::Bad).size(14));
        }
        row![
            self.sidebar(),
            scrollable(container(main).padding([28, 32]).width(Length::Fill))
                .style(style::scrollbar)
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .into()
    }

    /// The app name, what's happening now, the pages, and the sources' sync status.
    fn sidebar(&self) -> Element<'_, Message> {
        // What's happening, always on the readout: the session and its time left, or idle.
        let (name, time) = match self
            .status
            .as_ref()
            .and_then(|status| Some((status, status.session.running()?)))
        {
            Some((status, running)) => (
                running.spec().config.name.to_uppercase(),
                sessions::format_duration(running.time_left(status.now)),
            ),
            None => ("IDLE".to_string(), "--:--".to_string()),
        };
        let now_line = style::readout(
            row![
                style::mono(name).size(13),
                space::horizontal(),
                style::mono(time).size(13),
            ]
            .spacing(6),
        )
        .width(Length::Fill);
        let on_sessions = matches!(self.page, Page::Sessions | Page::Settings);
        let mut sources = column![].spacing(6);
        if let Some(status) = &self.status {
            for (name, source) in [
                ("Cal ", &status.plan.calendar),
                ("Todo", &status.plan.tasks),
            ] {
                let (label, tone) = match &source.state {
                    SourceState::Ok => ("synced", Tone::Good),
                    SourceState::Failing { offline: true, .. } => ("offline", Tone::Warn),
                    SourceState::Failing { .. } => ("problem", Tone::Bad),
                    SourceState::NotConfigured => ("not connected", Tone::Quiet),
                    SourceState::Unknown => ("checking", Tone::Quiet),
                };
                sources = sources.push(
                    row![dot(tone), style::label(&format!("{name} {label}"))]
                        .spacing(6)
                        .align_y(Alignment::Center),
                );
            }
            // Which browsers the extension protects (a browser without it is closed in sessions).
            for line in sessions::browser_lines(status) {
                sources = sources.push(
                    row![dot(line.tone), style::label(&line.text)]
                        .spacing(6)
                        .align_y(Alignment::Center),
                );
            }
        }
        let mut readouts = column![now_line].spacing(12);
        if let Some(next) = self
            .status
            .as_ref()
            .and_then(|status| self.next_start_readout(status))
        {
            readouts = readouts.push(next);
        }
        style::sidebar(
            column![
                style::brand(),
                readouts,
                space::vertical().height(Length::Fixed(12.0)),
                nav(
                    "Overview",
                    self.page == Page::Overview,
                    Message::Navigate(Page::Overview)
                ),
                nav("Sessions", on_sessions, Message::Navigate(Page::Sessions)),
                nav(
                    "Calendar",
                    self.page == Page::Calendar,
                    Message::Navigate(Page::Calendar)
                ),
                nav(
                    "Todoist",
                    self.page == Page::Todoist,
                    Message::Navigate(Page::Todoist)
                ),
                space::vertical(),
                sources,
            ]
            .spacing(8),
        )
        .into()
    }
}
