//! The Rules page: what sessions restrict (configurations and shared lists), what starts them by
//! itself, and the limits that apply to every session. It edits the same settings draft as the
//! Settings page, so both save the same way.

use iced::widget::{Column, column, row, space};
use iced::{Alignment, Element};
use ipc_proto::StatusReport;

use crate::configs::{self, Target};
use crate::help::info;
use crate::rules;
use crate::sessions::pending_summary;
use crate::settings::{Msg, Tab};
use crate::style::{self, caption, card, heading, title};
use crate::{App, Message};

impl App {
    /// The Rules page.
    pub(crate) fn rules_page<'a>(&'a self, status: &'a StatusReport) -> Element<'a, Message> {
        let mut header = row![
            title("Rules"),
            info(
                "What sessions restrict and what starts them. Edits to configurations in use \
                 wait 24 hours (cooling off)."
            ),
            space::horizontal(),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if let Some(draft) = &self.settings {
            header = header.push(self.save_status(draft));
        }
        let tabs = style::segmented(
            &[Tab::Configs, Tab::Lists, Tab::AutoStarts, Tab::Limits],
            self.settings_tab,
            |tab| {
                match tab {
                    Tab::Configs => "Configurations",
                    Tab::Lists => "Lists",
                    Tab::AutoStarts => "Automatic starts",
                    Tab::Limits => "Limits",
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
            return page.push(self.settings_unavailable()).into();
        };
        let body: Element<'a, Message> = match self.settings_tab {
            Tab::Configs => {
                configs::view(&draft.edited, &draft.configs, Target::Configs, &self.apps)
            }
            Tab::Lists => configs::view(&draft.edited, &draft.lists, Target::Lists, &self.apps),
            Tab::AutoStarts => rules::view(&draft.rules, &self.rule_configs),
            Tab::Limits => crate::friction::view(
                &draft.edited.friction,
                status.friction_loosen_after.map(|at| (at, status.now)),
            ),
        };
        page.push(body).into()
    }
}
