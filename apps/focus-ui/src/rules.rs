//! The editor for automatic-start rules: which configuration a calendar keyword or Todoist label
//! starts, and how strictly. Edits stay in the window until Save sends them to the daemon, which
//! writes them into `settings.toml`.

use std::time::Duration;

use focus_core::{AutoStartRule, Strictness};
use iced::widget::{Column, button, column, pick_list, row, slider, space, text, text_input};
use iced::{Alignment, Element, Length};

use crate::Message;
use crate::help::{info, tip};
use crate::style::{self, Tone, caption, toned};

/// Shortest and longest task session the slider offers, in minutes.
const TASK_MINUTES: std::ops::RangeInclusive<u16> = 5..=240;

/// One rule being edited, plus what's typed in its "add" boxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The rule as it will be saved.
    pub rule: AutoStartRule,
    keyword: String,
    label: String,
    /// Whether removing it is waiting for a yes.
    confirm_remove: bool,
}

impl Draft {
    /// A draft of a saved rule.
    pub fn from_rule(rule: AutoStartRule) -> Self {
        Self {
            rule,
            keyword: String::new(),
            label: String::new(),
            confirm_remove: false,
        }
    }

    /// A new rule for `config`, Deep, with nothing to start it yet.
    fn blank(config: String) -> Self {
        Self::from_rule(AutoStartRule {
            config,
            strictness: Strictness::Deep,
            calendar_keywords: Vec::new(),
            task_labels: Vec::new(),
            task_duration: AutoStartRule::DEFAULT_TASK_DURATION,
        })
    }
}

/// One change to the rules being edited.
#[derive(Debug, Clone)]
pub enum Edit {
    Add,
    AskRemove(usize),
    KeepRule(usize),
    Remove(usize),
    Config(usize, String),
    Strictness(usize, Strictness),
    TaskMinutes(usize, u16),
    KeywordTyped(usize, String),
    AddKeyword(usize),
    RemoveKeyword(usize, usize),
    LabelTyped(usize, String),
    AddLabel(usize),
    RemoveLabel(usize, usize),
}

/// Applies `edit` to the drafts. A new rule starts with the first of `configs`.
pub fn apply(drafts: &mut Vec<Draft>, edit: Edit, configs: &[String]) {
    if let Edit::Add = edit {
        drafts.push(Draft::blank(configs.first().cloned().unwrap_or_default()));
        return;
    }
    if let Edit::Remove(index) = edit {
        if index < drafts.len() {
            drafts.remove(index);
        }
        return;
    }
    let index = match &edit {
        Edit::AskRemove(i)
        | Edit::KeepRule(i)
        | Edit::Config(i, _)
        | Edit::Strictness(i, _)
        | Edit::TaskMinutes(i, _)
        | Edit::KeywordTyped(i, _)
        | Edit::AddKeyword(i)
        | Edit::RemoveKeyword(i, _)
        | Edit::LabelTyped(i, _)
        | Edit::AddLabel(i)
        | Edit::RemoveLabel(i, _) => *i,
        Edit::Add | Edit::Remove(_) => return,
    };
    let Some(draft) = drafts.get_mut(index) else {
        return;
    };
    match edit {
        Edit::AskRemove(_) => draft.confirm_remove = true,
        Edit::KeepRule(_) => draft.confirm_remove = false,
        Edit::Config(_, config) => draft.rule.config = config,
        Edit::Strictness(_, strictness) => draft.rule.strictness = strictness,
        Edit::TaskMinutes(_, minutes) => {
            draft.rule.task_duration = Duration::from_secs(u64::from(minutes) * 60);
        }
        Edit::KeywordTyped(_, typed) => draft.keyword = typed,
        Edit::AddKeyword(_) => add_entry(&mut draft.rule.calendar_keywords, &mut draft.keyword),
        Edit::RemoveKeyword(_, at) => remove_entry(&mut draft.rule.calendar_keywords, at),
        Edit::LabelTyped(_, typed) => draft.label = typed,
        Edit::AddLabel(_) => {
            draft.label = draft.label.trim().trim_start_matches('@').to_owned();
            add_entry(&mut draft.rule.task_labels, &mut draft.label);
        }
        Edit::RemoveLabel(_, at) => remove_entry(&mut draft.rule.task_labels, at),
        Edit::Add | Edit::Remove(_) => {}
    }
}

/// Moves what was typed into `entries`, unless it's blank or already there (ignoring case).
fn add_entry(entries: &mut Vec<String>, typed: &mut String) {
    let entry = typed.trim().to_owned();
    typed.clear();
    let duplicate = entries
        .iter()
        .any(|existing| existing.to_lowercase() == entry.to_lowercase());
    if !entry.is_empty() && !duplicate {
        entries.push(entry);
    }
}

fn remove_entry(entries: &mut Vec<String>, at: usize) {
    if at < entries.len() {
        entries.remove(at);
    }
}

/// Why the drafts can't be saved yet, worded for the window; `None` if they can.
pub fn problem(drafts: &[Draft], configs: &[String]) -> Option<String> {
    drafts.iter().enumerate().find_map(|(index, draft)| {
        let number = index + 1;
        if !configs.contains(&draft.rule.config) {
            Some(format!("Rule {number}: pick a configuration."))
        } else if draft.rule.calendar_keywords.is_empty() && draft.rule.task_labels.is_empty() {
            Some(format!(
                "Rule {number}: add a calendar keyword or a Todoist label, or it never starts."
            ))
        } else {
            None
        }
    })
}

/// The rules tab: one card per rule, then a button to add one. Saving is the Settings page's.
pub fn view<'a>(drafts: &'a [Draft], configs: &'a [String]) -> Element<'a, Message> {
    let mut section = column![
        row![
            caption("Calendar events and Todoist tasks that start sessions."),
            info(
                "An event whose title has a keyword runs its configuration until it ends. A task \
                 due at a time with one of the labels runs it for the task's duration."
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    ]
    .spacing(14);
    if drafts.is_empty() {
        section = section.push(caption(
            "No rules yet: nothing in your calendar or Todoist starts a session.",
        ));
    }
    for (index, draft) in drafts.iter().enumerate() {
        section = section.push(card(index, draft, configs));
    }
    let add = (!configs.is_empty()).then_some(Message::Rule(Edit::Add));
    section = section.push(button("Add rule").style(style::key).on_press_maybe(add));
    if configs.is_empty() {
        section = section.push(caption(
            "Create a configuration first (Configurations tab).",
        ));
    }
    if let Some(problem) = problem(drafts, configs) {
        section = section.push(toned(problem, Tone::Bad).size(14));
    }
    section.into()
}

/// One rule as a card.
fn card<'a>(index: usize, draft: &'a Draft, configs: &'a [String]) -> Element<'a, Message> {
    let rule = &draft.rule;
    let selected = configs.iter().find(|name| **name == rule.config);
    // Read as a sentence: when an event or task matches, run a configuration at a strictness.
    let when = row![
        style::field_label("When an event title has"),
        inline_entries(
            &rule.calendar_keywords,
            &draft.keyword,
            "+ keyword",
            move |typed| Message::Rule(Edit::KeywordTyped(index, typed)),
            Message::Rule(Edit::AddKeyword(index)),
            move |at| Message::Rule(Edit::RemoveKeyword(index, at)),
        ),
        style::field_label("or a task is labelled"),
        inline_entries(
            &rule.task_labels,
            &draft.label,
            "+ label",
            move |typed| Message::Rule(Edit::LabelTyped(index, typed)),
            Message::Rule(Edit::AddLabel(index)),
            move |at| Message::Rule(Edit::RemoveLabel(index, at)),
        ),
        info(
            "Events: the title contains a keyword, in any case (a tag like #focus is easy to \
             add). Tasks: one of the labels (no @) and due at a time of day. Press Enter to add."
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .wrap();
    let run = row![
        style::field_label("run"),
        pick_list(configs, selected, move |config| {
            Message::Rule(Edit::Config(index, config))
        })
        .placeholder("configuration")
        .style(style::picker)
        .menu_style(style::menu),
        style::field_label("at"),
        style::segmented(
            &Strictness::ALL,
            rule.strictness,
            |level| level.to_string(),
            move |level| Message::Rule(Edit::Strictness(index, level)),
        ),
        space::horizontal(),
        remove_control(index, draft.confirm_remove),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let minutes = u16::try_from(rule.task_duration.as_secs() / 60).unwrap_or(u16::MAX);
    let task_length = row![
        style::caption(format!("Tasks without a length run {minutes} min")),
        slider(
            TASK_MINUTES,
            minutes.clamp(*TASK_MINUTES.start(), *TASK_MINUTES.end()),
            move |minutes| Message::Rule(Edit::TaskMinutes(index, minutes))
        )
        .step(5u16)
        .width(Length::Fixed(220.0))
        .style(style::fader),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    style::card(column![when, run, task_length].spacing(14)).into()
}

/// A small × that asks before removing the rule.
fn remove_control<'a>(index: usize, confirming: bool) -> Element<'a, Message> {
    if confirming {
        row![
            toned("Remove this rule?", Tone::Bad).size(14),
            button("Remove")
                .style(style::key_danger)
                .on_press(Message::Rule(Edit::Remove(index))),
            button("Keep")
                .style(style::key)
                .on_press(Message::Rule(Edit::KeepRule(index))),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        tip(
            button(text("×").size(18))
                .style(style::key_flat)
                .on_press(Message::Rule(Edit::AskRemove(index))),
            "Remove this rule",
        )
    }
}

/// Removable chips followed by a small box that adds what's typed on Enter.
fn inline_entries<'a>(
    entries: &'a [String],
    typed: &'a str,
    placeholder: &'a str,
    on_type: impl Fn(String) -> Message + 'a,
    on_add: Message,
    on_remove: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    let add = (!typed.trim().is_empty()).then_some(on_add);
    entries
        .iter()
        .enumerate()
        .fold(
            row![].spacing(6).align_y(Alignment::Center),
            |chips, (at, entry)| chips.push(chip(entry, on_remove(at))),
        )
        .push(
            text_input(placeholder, typed)
                .style(style::field)
                .on_input(on_type)
                .on_submit_maybe(add)
                .width(Length::Fixed(110.0)),
        )
        .into()
}

/// One removable entry: its text and a ×; clicking removes it.
pub(crate) fn chip<'a>(entry: &str, on_remove: Message) -> Element<'a, Message> {
    tip(
        button(text(format!("{entry}  ×")).size(14))
            .style(style::key)
            .on_press(on_remove),
        "Click to remove.",
    )
}

/// A list of removable entries with a box and button to add more; shared with the
/// configuration editor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn entries<'a>(
    title: &'a str,
    explanation: &'a str,
    placeholder: &'a str,
    entries: &'a [String],
    typed: &'a str,
    on_type: impl Fn(String) -> Message + 'a,
    on_add: Message,
    on_remove: impl Fn(usize) -> Message + 'a,
) -> Column<'a, Message> {
    let mut chips = row![].spacing(6);
    for (at, entry) in entries.iter().enumerate() {
        chips = chips.push(chip(entry, on_remove(at)));
    }
    let add = (!typed.trim().is_empty()).then_some(on_add);
    column![
        row![style::field_label(title), info(explanation)]
            .spacing(8)
            .align_y(Alignment::Center),
        chips.wrap(),
        row![
            text_input(placeholder, typed)
                .style(style::field)
                .on_input(on_type)
                .on_submit_maybe(add.clone()),
            button("Add").style(style::key).on_press_maybe(add),
        ]
        .spacing(8),
    ]
    .spacing(6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs() -> Vec<String> {
        vec!["coding".into(), "writing".into()]
    }

    #[test]
    fn edits_build_a_rule() {
        let mut drafts = Vec::new();
        apply(&mut drafts, Edit::Add, &configs());
        assert_eq!(
            drafts[0].rule.config, "coding",
            "starts with the first config"
        );
        apply(
            &mut drafts,
            Edit::Strictness(0, Strictness::Locked),
            &configs(),
        );
        apply(
            &mut drafts,
            Edit::KeywordTyped(0, "  #focus ".into()),
            &configs(),
        );
        apply(&mut drafts, Edit::AddKeyword(0), &configs());
        apply(
            &mut drafts,
            Edit::KeywordTyped(0, "#FOCUS".into()),
            &configs(),
        );
        apply(&mut drafts, Edit::AddKeyword(0), &configs());
        apply(&mut drafts, Edit::LabelTyped(0, "@deep".into()), &configs());
        apply(&mut drafts, Edit::AddLabel(0), &configs());
        apply(&mut drafts, Edit::TaskMinutes(0, 25), &configs());
        let rule = &drafts[0].rule;
        assert_eq!(rule.strictness, Strictness::Locked);
        assert_eq!(rule.calendar_keywords, ["#focus"], "trimmed, no duplicates");
        assert_eq!(rule.task_labels, ["deep"], "without the @");
        assert_eq!(rule.task_duration, Duration::from_secs(25 * 60));
        assert!(drafts[0].keyword.is_empty(), "the box empties after adding");

        apply(&mut drafts, Edit::RemoveKeyword(0, 0), &configs());
        assert!(drafts[0].rule.calendar_keywords.is_empty());
        apply(&mut drafts, Edit::Remove(0), &configs());
        assert!(drafts.is_empty());
        apply(&mut drafts, Edit::Remove(3), &configs());
    }

    #[test]
    fn problems_block_saving() {
        let mut drafts = vec![Draft::blank("coding".into())];
        assert_eq!(
            problem(&drafts, &configs()).as_deref(),
            Some("Rule 1: add a calendar keyword or a Todoist label, or it never starts.")
        );
        drafts[0].rule.task_labels.push("deep".into());
        assert_eq!(problem(&drafts, &configs()), None);
        drafts[0].rule.config = "gone".into();
        assert_eq!(
            problem(&drafts, &configs()).as_deref(),
            Some("Rule 1: pick a configuration.")
        );
    }
}
