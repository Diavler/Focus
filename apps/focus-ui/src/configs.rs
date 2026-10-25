//! Editing block configurations and shared lists: their URL patterns, and for configurations the
//! mode, included lists and apps to close. Patterns are checked as they're added, and a URL can
//! be tried against the edited configuration before saving.

use focus_core::{
    ConfigEntry, EditableSettings, ListEntry, Mode, RulePatterns, UrlPattern, same_app,
};
use iced::widget::{
    Column, button, checkbox, column, pick_list, radio, row, space, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::help::info;
use crate::rules::entries;
use crate::style::{Tone, caption, card, heading, toned};
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

/// How patterns are written, for the "(?)" next to each group.
const PATTERN_HELP: &str = "site.com covers the whole site and its subdomains; site.com/ is \
                            only the front page; site.com/path/* matches anything under that \
                            path. No https:// or query strings needed.";

/// The state of one editor: what's selected and what's typed.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    selected: Option<String>,
    new_name: String,
    typed: [String; 5],
    app: String,
    try_url: String,
    confirm_delete: bool,
    problem: Option<String>,
}

/// One change in an editor.
#[derive(Debug, Clone)]
pub enum Edit {
    Select(String),
    NewName(String),
    Create,
    Delete,
    ConfirmDelete(bool),
    Mode(Mode),
    Include(String, bool),
    Typed(usize, String),
    Add(usize),
    Remove(usize, usize),
    AppTyped(String),
    AddApp,
    PickApp(String),
    RemoveApp(usize),
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
                new_name: std::mem::take(&mut editor.new_name),
                ..Editor::default()
            };
        }
        Edit::NewName(name) => editor.new_name = name,
        Edit::Create => create(settings, editor, target),
        Edit::Delete => editor.confirm_delete = true,
        Edit::ConfirmDelete(false) => editor.confirm_delete = false,
        Edit::ConfirmDelete(true) => return delete(settings, editor, target),
        Edit::TryUrl(url) => editor.try_url = url,
        Edit::Typed(slot, typed) => {
            if let Some(field) = editor.typed.get_mut(slot) {
                *field = typed;
            }
        }
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
        Edit::Add(slot) => {
            let Some(verdict) = focus_core::ACTIONS.get(slot).copied() else {
                return;
            };
            let typed = editor.typed[slot].trim().to_owned();
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
            editor.typed[slot].clear();
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
            "A configuration is what a session runs: which pages are blocked, delayed, asked \
             about or have their feeds hidden, and which apps are closed. To rename one, create \
             a new one and delete the old.",
        ),
        Target::Lists => (
            "list",
            "Shared lists are groups of sites (say, social media) that several configurations \
             can include, so you only keep them up to date in one place.",
        ),
    };
    let create = (!editor.new_name.trim().is_empty()).then(|| wrap(Edit::Create));
    let picker = row![
        pick_list(names, current.clone(), move |name| wrap(Edit::Select(name)))
            .placeholder(format!("No {noun}s yet")),
        info(explanation),
        space::horizontal(),
        text_input(&format!("New {noun} name"), &editor.new_name)
            .on_input(move |name| wrap(Edit::NewName(name)))
            .on_submit_maybe(create.clone())
            .width(Length::Fixed(200.0)),
        button("Create").on_press_maybe(create),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let mut page = column![picker].spacing(16);
    if let Some(problem) = &editor.problem {
        page = page.push(toned(problem.as_str(), Tone::Bad).size(14));
    }
    let Some(name) = current else {
        return page
            .push(caption(format!(
                "No {noun}s yet. Type a name above and press Create."
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

    let delete: Element<'a, Message> = if editor.confirm_delete {
        row![
            toned(format!("Delete \"{name}\"?"), Tone::Bad).size(14),
            button("Delete")
                .style(button::danger)
                .on_press(wrap(Edit::ConfirmDelete(true))),
            button("Keep")
                .style(button::secondary)
                .on_press(wrap(Edit::ConfirmDelete(false))),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        button("Delete")
            .style(button::danger)
            .on_press(wrap(Edit::Delete))
            .into()
    };
    let mut body: Column<'a, Message> = column![
        row![heading(name.clone()), space::horizontal(), delete]
            .spacing(10)
            .align_y(Alignment::Center)
    ]
    .spacing(18);

    if let Some(config) = config {
        body = body.push(mode_picker(config, wrap));
        body = body.push(include_picker(settings, config, wrap));
    }
    for (slot, title, help) in ACTION_ROWS {
        let entries_here = patterns.get(focus_core::ACTIONS[slot]);
        body = body.push(entries(
            title,
            help,
            "site.com or site.com/path/*",
            entries_here,
            &editor.typed[slot],
            move |typed| wrap(Edit::Typed(slot, typed)),
            wrap(Edit::Add(slot)),
            move |at| wrap(Edit::Remove(slot, at)),
        ));
    }
    body = body.push(caption(PATTERN_HELP));
    if let Some(config) = config {
        body = body.push(apps_editor(config, editor, open_apps, wrap));
        body = body.push(try_url(settings, editor, &config.name, wrap));
    }
    page.push(card(body)).into()
}

fn mode_picker<'a>(
    config: &'a ConfigEntry,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    column![
        row![
            text("Mode").size(14),
            info(
                "Blocklist: everything opens except what the rules restrict. Allowlist: only \
                 the Allow patterns open; everything else is blocked (good for writing or \
                 studying with a few reference sites)."
            ),
        ]
        .spacing(8),
        row![
            radio(
                "Blocklist: block what's listed",
                Mode::Blocklist,
                Some(config.mode),
                move |mode| wrap(Edit::Mode(mode))
            ),
            radio(
                "Allowlist: allow only what's listed",
                Mode::Allowlist,
                Some(config.mode),
                move |mode| wrap(Edit::Mode(mode))
            ),
        ]
        .spacing(20),
    ]
    .spacing(8)
    .into()
}

fn include_picker<'a>(
    settings: &'a EditableSettings,
    config: &'a ConfigEntry,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let mut lists = row![].spacing(16);
    for list in &settings.lists {
        let name = list.name.clone();
        lists = lists.push(
            checkbox(config.include.contains(&list.name))
                .label(list.name.as_str())
                .on_toggle(move |on| wrap(Edit::Include(name.clone(), on))),
        );
    }
    let body: Element<'a, Message> = if settings.lists.is_empty() {
        caption("No shared lists yet; make them in the Lists tab.").into()
    } else {
        lists.wrap().into()
    };
    column![
        row![
            text("Includes").size(14),
            info("Shared lists whose rules this configuration uses too.")
        ]
        .spacing(8),
        body,
    ]
    .spacing(8)
    .into()
}

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
            "Closed during a session, by program name (without .exe). Each is asked to close \
             first, so it can save your work, then ended if it's still open a few seconds \
             later. Focus never closes itself or Windows' own programs.",
            "program name, e.g. discord",
            &config.apps,
            &editor.app,
            move |typed| wrap(Edit::AppTyped(typed)),
            wrap(Edit::AddApp),
            move |at| wrap(Edit::RemoveApp(at)),
        ),
        pick_list(offer, None::<String>, move |app| wrap(Edit::PickApp(app)))
            .placeholder("…or pick an app that's open now"),
    ]
    .spacing(8)
    .into()
}

fn try_url<'a>(
    settings: &'a EditableSettings,
    editor: &'a Editor,
    name: &str,
    wrap: impl Fn(Edit) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let url = editor.try_url.trim();
    let result = match settings.resolve(name) {
        _ if url.is_empty() => {
            "Type an address to see what this configuration does with it (unsaved edits count)."
                .to_owned()
        }
        Some(config) => format!("{}", config.verdict(url)),
        None => String::new(),
    };
    column![
        row![
            text("Try a URL").size(14),
            info("Checks an address against the rules above, including edits not saved yet. Nothing is opened."),
        ]
        .spacing(8),
        text_input("reddit.com/r/rust/comments/abc", &editor.try_url)
            .on_input(move |url| wrap(Edit::TryUrl(url))),
        caption(result),
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
            Edit::Typed(4, " x.com ".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::Add(4));
        apply(
            &mut settings,
            &mut editor,
            configs,
            Edit::Typed(4, "*.bad.com".into()),
        );
        apply(&mut settings, &mut editor, configs, Edit::Add(4));
        assert!(editor.problem.is_some(), "a bad pattern is explained");
        assert_eq!(editor.typed[4], "*.bad.com", "and kept for fixing");
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
