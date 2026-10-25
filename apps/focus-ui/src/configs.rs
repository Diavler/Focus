//! Editing block configurations and shared lists: their URL patterns, and for configurations the
//! mode, included lists and apps to close. Patterns are checked as they're added, and a URL can
//! be tried against the edited configuration before saving.

use focus_core::{
    ConfigEntry, EditableSettings, ListEntry, Mode, RulePatterns, UrlPattern, same_app,
};
use iced::widget::{
    Column, button, checkbox, column, container, pick_list, row, space, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::help::{info, tip};
use crate::rules::{chip, entries};
use crate::style::{self, Tone, caption, card, heading, toned};
use crate::{Message, settings};

/// Which kind of thing an editor edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Block configurations.
    Configs,
    /// Shared lists.
    Lists,
}

/// The actions in the order the window shows them (strictest first), as indexes into
/// [`focus_core::ACTIONS`], with their names and explanations.
const ACTION_ROWS: [(usize, &str, &str); 5] = [
    (
        4,
        "Block",
        "Pages matching these don't open at all during a session.",
    ),
    (
        3,
        "Hide feeds",
        "The site works, but feeds and recommendations are hidden (Reddit and YouTube for now), \
         so a single post or video is fine and endless scrolling isn't.",
    ),
    (
        2,
        "Delay",
        "Opens after a countdown that grows with each visit, so a quick check costs something.",
    ),
    (
        1,
        "Ask",
        "Asks whether the page is part of what you're working on before it opens.",
    ),
    (
        0,
        "Allow",
        "Always allowed, even inside a blocked site (e.g. reddit.com/r/rust/comments/* while \
         reddit.com hides feeds). In an allowlist, these are the only pages that open.",
    ),
];

/// How patterns are written, for the "(?)" beside the add bar.
const PATTERN_HELP: &str = "site.com covers the whole site and its subdomains; site.com/ is \
                            only the front page; site.com/path/* matches anything under that \
                            path. No https:// or query strings needed.";

/// The state of one editor: what's selected and what's typed.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    selected: Option<String>,
    /// Whether the name box for a new one is showing.
    naming: bool,
    new_name: String,
    /// The site typed in the add bar, and what it does.
    site: String,
    action: Action,
    app: String,
    /// Whether the URL tester is open.
    testing: bool,
    try_url: String,
    confirm_delete: bool,
    problem: Option<String>,
}

/// An entry in the configuration or list dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Choice {
    /// One that exists, by name.
    Existing(String),
    /// Make a new one; holds the noun ("configuration" or "list").
    New(&'static str),
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Existing(name) => f.write_str(name),
            Self::New(noun) => write!(f, "+ New {noun}"),
        }
    }
}

/// What a site in the add bar does: an index into [`ACTION_ROWS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Action(usize);

impl Action {
    /// Every action, strictest first, as the add bar offers them.
    const ALL: [Self; 5] = [Self(0), Self(1), Self(2), Self(3), Self(4)];

    /// Its index into [`focus_core::ACTIONS`].
    fn slot(self) -> usize {
        ACTION_ROWS.get(self.0).map_or(4, |row| row.0)
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(ACTION_ROWS.get(self.0).map_or("Block", |row| row.1))
    }
}

/// One change in an editor.
#[derive(Debug, Clone)]
pub enum Edit {
    Select(String),
    StartNew,
    CancelNew,
    NewName(String),
    Create,
    Delete,
    ConfirmDelete(bool),
    Mode(Mode),
    Include(String, bool),
    SiteTyped(String),
    Action(Action),
    Add,
    Remove(usize, usize),
    AppTyped(String),
    AddApp,
    PickApp(String),
    RemoveApp(usize),
    ToggleTest,
    TryUrl(String),
}

/// Something an edit deleted, so settings that refer to it can follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// A configuration (automatic-start rules using it go too).
    Config(String),
}

/// Applies `edit` to `settings`.
pub fn apply(
    settings: &mut EditableSettings,
    editor: &mut Editor,
    target: Target,
    edit: Edit,
) -> Option<Removed> {
    editor.problem = None;
    match edit {
        Edit::Select(name) => {
            *editor = Editor {
                selected: Some(name),
                naming: editor.naming,
                new_name: std::mem::take(&mut editor.new_name),
                action: editor.action,
                ..Editor::default()
            };
        }
        Edit::StartNew => editor.naming = true,
        Edit::CancelNew => {
            editor.naming = false;
            editor.new_name.clear();
        }
        Edit::NewName(name) => editor.new_name = name,
        Edit::Create => create(settings, editor, target),
        Edit::Delete => editor.confirm_delete = true,
        Edit::ConfirmDelete(false) => editor.confirm_delete = false,
        Edit::ConfirmDelete(true) => return delete(settings, editor, target),
        Edit::ToggleTest => editor.testing = !editor.testing,
        Edit::TryUrl(url) => editor.try_url = url,
        Edit::SiteTyped(site) => editor.site = site,
        Edit::Action(action) => editor.action = action,
        Edit::AppTyped(typed) => editor.app = typed,
        other => {
            let name = selected(settings, editor, target)?;
            edit_selected(settings, editor, target, &name, other);
        }
    }
    None
}

fn create(settings: &mut EditableSettings, editor: &mut Editor, target: Target) {
    let name = editor.new_name.trim().to_owned();
    if name.is_empty() {
        editor.problem = Some("Type a name first.".into());
        return;
    }
    let taken = match target {
        Target::Configs => settings.configs.iter().any(|config| config.name == name),
        Target::Lists => settings.lists.iter().any(|list| list.name == name),
    };
    if taken {
        editor.problem = Some(format!("There's already one named \"{name}\"."));
        return;
    }
    match target {
        Target::Configs => {
            settings.configs.push(ConfigEntry {
                name: name.clone(),
                mode: Mode::Blocklist,
                include: Vec::new(),
                patterns: RulePatterns::default(),
                apps: Vec::new(),
            });
            settings.configs.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Target::Lists => {
            settings.lists.push(ListEntry {
                name: name.clone(),
                patterns: RulePatterns::default(),
            });
            settings.lists.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
    *editor = Editor {
        selected: Some(name),
        ..Editor::default()
    };
}

fn delete(settings: &mut EditableSettings, editor: &mut Editor, target: Target) -> Option<Removed> {
    let name = selected(settings, editor, target)?;
    *editor = Editor::default();
    match target {
        Target::Configs => {
            settings.configs.retain(|config| config.name != name);
            Some(Removed::Config(name))
        }
        Target::Lists => {
            settings.lists.retain(|list| list.name != name);
            // Configurations stop including it, so the settings stay valid.
            for config in &mut settings.configs {
                config.include.retain(|included| *included != name);
            }
            None
        }
    }
}

fn edit_selected(
    settings: &mut EditableSettings,
    editor: &mut Editor,
    target: Target,
    name: &str,
    edit: Edit,
) {
    let config = settings
        .configs
        .iter_mut()
        .find(|config| config.name == name);
    let patterns = match target {
        Target::Configs => config.map(|config| &mut config.patterns),
        Target::Lists => settings
            .lists
            .iter_mut()
            .find(|list| list.name == name)
            .map(|list| &mut list.patterns),
    };
    let Some(patterns) = patterns else { return };
    match edit {
        Edit::Add => {
            let Some(verdict) = focus_core::ACTIONS.get(editor.action.slot()).copied() else {
                return;
            };
            let typed = editor.site.trim().to_owned();
            if typed.is_empty() {
                return;
            }
            if let Err(error) = UrlPattern::parse(&typed) {
                editor.problem = Some(format!("\"{typed}\": {error}"));
                return;
            }
            let list = patterns.get_mut(verdict);
            if !list.contains(&typed) {
                list.push(typed);
            }
            editor.site.clear();
        }
        Edit::Remove(slot, at) => {
            if let Some(verdict) = focus_core::ACTIONS.get(slot).copied() {
                let list = patterns.get_mut(verdict);
                if at < list.len() {
                    list.remove(at);
                }
            }
        }
        other => {
            // The rest only apply to configurations.
            let Some(config) = settings
                .configs
                .iter_mut()
                .find(|config| config.name == name)
            else {
                return;
            };
            match other {
                Edit::Mode(mode) => config.mode = mode,
                Edit::Include(list, on) => {
                    config.include.retain(|included| *included != list);
                    if on {
                        config.include.push(list);
                    }
                }
                Edit::AddApp => {
                    let app = std::mem::take(&mut editor.app);
                    add_app(&mut config.apps, &app);
                }
                Edit::PickApp(app) => add_app(&mut config.apps, &app),
                Edit::RemoveApp(at) if at < config.apps.len() => {
                    config.apps.remove(at);
                }
                _ => {}
            }
        }
    }
}

/// Adds an app by executable name (without `.exe`, keeping its case), unless it's blank or
/// already there by the core's matching ([`same_app`]).
fn add_app(apps: &mut Vec<String>, app: &str) {
    let app = app.trim();
    let app = app
        .len()
        .checked_sub(".exe".len())
        .filter(|&cut| {
            app.get(cut..)
                .is_some_and(|end| end.eq_ignore_ascii_case(".exe"))
        })
        .and_then(|cut| app.get(..cut))
        .unwrap_or(app);
    if !app.is_empty() && !apps.iter().any(|existing| same_app(existing, app)) {
        apps.push(app.to_owned());
    }
}

/// The selected entry's name, or the first one if nothing (or something deleted) is selected.
fn selected(settings: &EditableSettings, editor: &Editor, target: Target) -> Option<String> {
    let names: Vec<&String> = match target {
        Target::Configs => settings.configs.iter().map(|config| &config.name).collect(),
        Target::Lists => settings.lists.iter().map(|list| &list.name).collect(),
    };
    editor
        .selected
        .as_ref()
        .filter(|name| names.contains(name))
        .or(names.first().copied())
        .cloned()
}

/// The editor for configurations or lists.
pub fn view<'a>(
    settings: &'a EditableSettings,
    editor: &'a Editor,
    target: Target,
    open_apps: &'a [String],
) -> Element<'a, Message> {
    let wrap = move |edit: Edit| match target {
        Target::Configs => Message::Settings(settings::Msg::Config(edit)),
        Target::Lists => Message::Settings(settings::Msg::List(edit)),
    };
    let names: Vec<String> = match target {
        Target::Configs => settings.configs.iter().map(|c| c.name.clone()).collect(),
        Target::Lists => settings.lists.iter().map(|l| l.name.clone()).collect(),
    };
    let current = selected(settings, editor, target);
    let (noun, explanation) = match target {
        Target::Configs => (
            "configuration",
            "What a session runs: which sites are restricted and which apps are closed. To \
             rename one, create a new one and delete the old.",
        ),
        Target::Lists => (
            "list",
            "A group of sites (say, social media) that several configurations can include.",
        ),
    };
    // The dropdown lists the existing ones, then a way to make another.
    let choices: Vec<Choice> = names
        .into_iter()
        .map(Choice::Existing)
        .chain([Choice::New(noun)])
        .collect();
    let mut picker = row![
        pick_list(
            choices,
            current.clone().map(Choice::Existing),
            move |choice| match choice {
                Choice::Existing(name) => wrap(Edit::Select(name)),
                Choice::New(_) => wrap(Edit::StartNew),
            }
        )
        .placeholder(format!("No {noun}s yet"))
        .style(style::picker)
        .menu_style(style::menu),
        info(explanation),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if editor.naming {
        let create = (!editor.new_name.trim().is_empty()).then(|| wrap(Edit::Create));
        picker = picker
            .push(
                text_input(&format!("New {noun} name"), &editor.new_name)
                    .style(style::field)
                    .on_input(move |name| wrap(Edit::NewName(name)))
                    .on_submit_maybe(create.clone())
                    .width(Length::Fixed(200.0)),
            )
            .push(
                button("Create")
                    .style(style::key_accent)
                    .on_press_maybe(create),
            )
            .push(
                button("Cancel")
                    .style(style::key)
                    .on_press(wrap(Edit::CancelNew)),
            );
    }
    let mut page = column![picker].spacing(16);
    // A problem with the new name shows by its box; others by the add bar.
    let problem = editor
        .problem
        .as_deref()
        .map(|problem| toned(problem, Tone::Bad).size(14));
    let (name_problem, site_problem) = if editor.naming {
        (problem, None)
    } else {
        (None, problem)
    };
    if let Some(problem) = name_problem {
        page = page.push(problem);
    }
    let Some(name) = current else {
        return page
            .push(caption(format!(
                "No {noun}s yet. Pick + New {noun} in the list to make one."
            )))
            .into();
    };
    let (patterns, config) = match target {
        Target::Configs => {
            let Some(config) = settings.configs.iter().find(|c| c.name == name) else {
                return page.into();
            };
            (&config.patterns, Some(config))
        }
        Target::Lists => {
            let Some(list) = settings.lists.iter().find(|l| l.name == name) else {
                return page.into();
            };
            (&list.patterns, None)
        }
    };

    let mut sites: Column<'a, Message> = column![
        row![
            heading(name.clone()),
            space::horizontal(),
            delete_key(editor, &name, wrap)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    ]
    .spacing(16);
    if let Some(config) = config {
        sites = sites.push(mode_and_includes(settings, config, wrap));
    }
    sites = sites.push(add_bar(editor, wrap));
    if let Some(problem) = site_problem {
        sites = sites.push(problem);
    }
    sites = sites.push(grouped_sites(patterns, wrap));
    if let Some(config) = config {
        sites = sites.push(try_url(settings, editor, &config.name, wrap));
    }
    page = page.push(card(sites));
    if let Some(config) = config {
        page = page.push(card(apps_editor(config, editor, open_apps, wrap)));
    }
    page.into()
}

/// Delete, asking first.
fn delete_key<'a>(
    editor: &Editor,
    name: &str,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    if editor.confirm_delete {
        row![
            toned(format!("Delete \"{name}\"?"), Tone::Bad).size(14),
            button("Delete")
                .style(style::key_danger)
                .on_press(wrap(Edit::ConfirmDelete(true))),
            button("Keep")
                .style(style::key)
                .on_press(wrap(Edit::ConfirmDelete(false))),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        // Plain until asked; the red key is the confirmation.
        button("Delete…")
            .style(style::key)
            .on_press(wrap(Edit::Delete))
            .into()
    }
}

/// Blocklist or allowlist, and the shared lists it includes, on one line.
fn mode_and_includes<'a>(
    settings: &'a EditableSettings,
    config: &'a ConfigEntry,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let mut line = row![
        style::segmented(
            &[Mode::Blocklist, Mode::Allowlist],
            config.mode,
            |mode| match mode {
                Mode::Blocklist => "Blocklist".to_owned(),
                Mode::Allowlist => "Allowlist".to_owned(),
            },
            move |mode| wrap(Edit::Mode(mode)),
        ),
        info(
            "Blocklist: everything opens except what's restricted below. Allowlist: only the \
             Allow sites open."
        ),
        space::horizontal().width(Length::Fixed(16.0)),
        style::field_label("Includes"),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if settings.lists.is_empty() {
        line = line.push(caption("no shared lists yet"));
    }
    for list in &settings.lists {
        let name = list.name.clone();
        line = line.push(
            checkbox(config.include.contains(&list.name))
                .label(list.name.as_str())
                .text_size(14)
                .on_toggle(move |on| wrap(Edit::Include(name.clone(), on))),
        );
    }
    line.push(info(
        "Shared lists whose sites this configuration uses too.",
    ))
    .wrap()
    .into()
}

/// One box for a site, what it does, and Add.
fn add_bar<'a>(
    editor: &'a Editor,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let add = (!editor.site.trim().is_empty()).then(|| wrap(Edit::Add));
    row![
        text_input("site.com or site.com/path/*", &editor.site)
            .style(style::field)
            .on_input(move |site| wrap(Edit::SiteTyped(site)))
            .on_submit_maybe(add.clone()),
        pick_list(Action::ALL, Some(editor.action), move |action| {
            wrap(Edit::Action(action))
        })
        .style(style::picker)
        .menu_style(style::menu),
        info(PATTERN_HELP),
        button("Add").style(style::key_accent).on_press_maybe(add),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

/// The sites under each action that has any, strictest first; hover an action for what it does.
fn grouped_sites<'a>(
    patterns: &'a RulePatterns,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let mut groups = column![].spacing(10);
    let mut any = false;
    for (slot, title, help) in ACTION_ROWS {
        let here = patterns.get(focus_core::ACTIONS[slot]);
        if here.is_empty() {
            continue;
        }
        any = true;
        let chips = here
            .iter()
            .enumerate()
            .fold(row![].spacing(6), |chips, (at, site)| {
                chips.push(chip(site, wrap(Edit::Remove(slot, at))))
            });
        groups = groups.push(
            row![
                container(tip(style::field_label(title), help)).width(Length::Fixed(100.0)),
                chips.wrap(),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    if any {
        groups.into()
    } else {
        caption("No sites yet. Add one above.").into()
    }
}

/// Apps to close during a session: typed, or picked from what's open now.
fn apps_editor<'a>(
    config: &'a ConfigEntry,
    editor: &'a Editor,
    open_apps: &'a [String],
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let offer: Vec<String> = open_apps
        .iter()
        .filter(|app| !config.apps.iter().any(|added| same_app(added, app)))
        .cloned()
        .collect();
    column![
        entries(
            "Apps to close",
            "Closed during a session, by program name (no .exe). Each is asked to close first, \
             then ended if it's still open. Focus never closes itself or Windows.",
            "program name, e.g. discord",
            &config.apps,
            &editor.app,
            move |typed| wrap(Edit::AppTyped(typed)),
            wrap(Edit::AddApp),
            move |at| wrap(Edit::RemoveApp(at)),
        ),
        pick_list(offer, None::<String>, move |app| wrap(Edit::PickApp(app)))
            .placeholder("…or pick an app that's open now")
            .style(style::picker)
            .menu_style(style::menu),
    ]
    .spacing(10)
    .into()
}

/// A closed "Test a URL" line that opens into a box and what the rules do with the address.
fn try_url<'a>(
    settings: &'a EditableSettings,
    editor: &'a Editor,
    name: &str,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let toggle = button(
        text(if editor.testing {
            "▾ Test a URL"
        } else {
            "▸ Test a URL"
        })
        .size(14),
    )
    .style(style::key_flat)
    .on_press(wrap(Edit::ToggleTest));
    if !editor.testing {
        return toggle.into();
    }
    let url = editor.try_url.trim();
    let result = match settings.resolve(name) {
        _ if url.is_empty() => String::new(),
        Some(config) => format!("{}", config.verdict(url)),
        None => String::new(),
    };
    column![
        toggle,
        row![
            text_input("reddit.com/r/rust/comments/abc", &editor.try_url)
                .style(style::field)
                .on_input(move |url| wrap(Edit::TryUrl(url))),
            info("What this configuration does with the address, unsaved edits included."),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        style::field_label(result),
    ]
    .spacing(8)
    .into()
}

#[cfg(test)]
mod tests {
    use focus_core::{RuleAction, Verdict};

    use super::*;

    fn settings() -> EditableSettings {
        EditableSettings {
            start_at_login: true,
            lists: vec![ListEntry {
                name: "social".into(),
                patterns: RulePatterns::default(),
            }],
            configs: vec![ConfigEntry {
                name: "coding".into(),
                mode: Mode::Blocklist,
                include: vec!["social".into()],
                patterns: RulePatterns::default(),
                apps: Vec::new(),
            }],
            auto_start: Vec::new(),
            friction: focus_core::Friction::default(),
            parking_hotkey: focus_core::Hotkey::default(),
            theme: focus_core::Theme::default(),
            revision: "1".into(),
        }
    }

    #[test]
    fn adds_checked_patterns_and_apps() {
        let mut settings = settings();
        let mut editor = Editor::default();
        let configs = Target::Configs;
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::SiteTyped(" x.com ".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::Add);
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::SiteTyped("*.bad.com".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::Add);
        assert!(editor.problem.is_some(), "a bad pattern is explained");
        assert_eq!(editor.site, "*.bad.com", "and kept for fixing");
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::AppTyped("Discord.Exe".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::AddApp);
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::PickApp("discord".into()),
        );
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::Mode(Mode::Allowlist),
        );
        let coding = &settings.configs[0];
        assert_eq!(coding.patterns.block, ["x.com"]);
        assert_eq!(coding.apps, ["Discord"], "no .exe, no duplicate");
        assert_eq!(coding.mode, Mode::Allowlist);
        assert_eq!(
            settings
                .resolve("coding")
                .unwrap()
                .verdict("https://x.com/"),
            Verdict::Restrict(RuleAction::Block)
        );
    }

    #[test]
    fn the_add_bar_files_a_site_under_the_picked_action() {
        let mut settings = settings();
        let mut editor = Editor::default();
        let configs = Target::Configs;
        assert_eq!(
            editor.action.to_string(),
            "Block",
            "blocking is the default"
        );
        apply(&mut settings, &mut editor, configs, Edit::Action(Action(4)));
        assert_eq!(editor.action.to_string(), "Allow");
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::SiteTyped("docs.rs".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::Add);
        assert!(
            settings.configs[0]
                .patterns
                .allow
                .contains(&"docs.rs".to_owned())
        );
        assert!(editor.site.is_empty());
        assert_eq!(editor.action.to_string(), "Allow", "kept for the next site");
    }

    #[test]
    fn the_name_box_shows_only_while_making_one() {
        let mut settings = settings();
        let mut editor = Editor::default();
        assert!(!editor.naming);
        apply(&mut settings, &mut editor, Target::Lists, Edit::StartNew);
        apply(
            &mut settings,
            &mut editor,
            Target::Lists,
            Edit::NewName("news".into()),
        );
        apply(&mut settings, &mut editor, Target::Lists, Edit::CancelNew);
        assert!(!editor.naming && editor.new_name.is_empty());
        apply(&mut settings, &mut editor, Target::Lists, Edit::StartNew);
        apply(
            &mut settings,
            &mut editor,
            Target::Lists,
            Edit::NewName("news".into()),
        );
        apply(&mut settings, &mut editor, Target::Lists, Edit::Create);
        assert!(!editor.naming, "closes once made");
        assert!(settings.lists.iter().any(|list| list.name == "news"));
    }

    #[test]
    fn creates_and_deletes() {
        let mut settings = settings();
        let mut editor = Editor::default();
        apply(
            &mut settings,
            &mut editor,
            Target::Configs,
            Edit::NewName("coding".into()),
        );
        apply(&mut settings, &mut editor, Target::Configs, Edit::Create);
        assert!(editor.problem.is_some(), "names are unique");
        apply(
            &mut settings,
            &mut editor,
            Target::Configs,
            Edit::NewName(" art ".into()),
        );
        apply(&mut settings, &mut editor, Target::Configs, Edit::Create);
        assert_eq!(settings.configs[0].name, "art", "sorted");
        assert_eq!(editor.selected.as_deref(), Some("art"));
        apply(&mut settings, &mut editor, Target::Configs, Edit::Delete);
        let removed = apply(
            &mut settings,
            &mut editor,
            Target::Configs,
            Edit::ConfirmDelete(true),
        );
        assert_eq!(removed, Some(Removed::Config("art".into())));

        let mut lists = Editor::default();
        apply(
            &mut settings,
            &mut lists,
            Target::Lists,
            Edit::ConfirmDelete(true),
        );
        assert!(settings.lists.is_empty());
        assert!(settings.configs[0].include.is_empty(), "no longer included");
    }
}
