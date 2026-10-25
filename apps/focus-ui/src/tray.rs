//! The tray icon: closing the window hides it there and clicking the icon brings it back. There's
//! no quit that only closes the window: Focus is either hidden (still working) or closed through
//! the Stop Focus section in Settings (the emergency exit), which the menu opens. With no session that's "Close Focus" (no wait);
//! during one it's "Stop Focus…", which waits. Windows only for now; Linux gets `ksni` in
//! step 9, and until then closing the window minimises it.

use iced::Subscription;

use crate::Message;

/// What the tray asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "no tray outside Windows until step 9")
)]
pub(crate) enum TrayAction {
    /// Come back from the tray.
    Show,
    /// Start closing Focus through the Stop Focus section in Settings.
    StopFocus,
}

/// The icon in the notification area; it goes away when this is dropped.
pub(crate) struct Tray {
    #[cfg(windows)]
    _icon: tray_icon::TrayIcon,
    /// The menu entry that closes or stops Focus; its label follows the session.
    #[cfg(windows)]
    stop: tray_icon::menu::MenuItem,
}

/// The menu entries' ids.
#[cfg(windows)]
const OPEN: &str = "open";
#[cfg(windows)]
const STOP: &str = "stop";

impl Tray {
    /// Adds the icon, or `None` if it couldn't be added. Call it on the thread that runs the
    /// window's event loop, which delivers the tray's clicks.
    #[cfg(windows)]
    pub(crate) fn create() -> Option<Self> {
        use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

        let menu = Menu::new();
        let stop = MenuItem::with_id(STOP, stop_label(false), true, None);
        menu.append_items(&[
            &MenuItem::with_id(OPEN, "Open Focus", true, None),
            &PredefinedMenuItem::separator(),
            &stop,
        ])
        .ok()?;
        let icon = tray_icon::Icon::from_rgba(crate::icon::rgba(32), 32, 32).ok()?;
        let icon = tray_icon::TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Focus keeps running in the background")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        Some(Self { _icon: icon, stop })
    }

    /// Names the stop entry for whether a session is `running`.
    #[cfg(windows)]
    pub(crate) fn show_session(&self, running: bool) {
        self.stop.set_text(stop_label(running));
    }

    /// Nothing to name without a tray.
    #[cfg(not(windows))]
    pub(crate) fn show_session(&self, _running: bool) {}

    /// There's no tray here yet.
    #[cfg(not(windows))]
    pub(crate) fn create() -> Option<Self> {
        None
    }
}

/// Clicks on the icon and its menu.
#[cfg(windows)]
pub(crate) fn subscription() -> Subscription<Message> {
    Subscription::run(actions).map(Message::Tray)
}

/// No tray, so nothing to hear.
#[cfg(not(windows))]
pub(crate) fn subscription() -> Subscription<Message> {
    Subscription::none()
}

/// The tray's clicks as a stream. `tray-icon` calls its handlers on the event loop's thread, so
/// they only pass each click on through a channel.
#[cfg(windows)]
fn actions() -> impl iced::futures::Stream<Item = TrayAction> {
    use tray_icon::menu::MenuEvent;
    use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

    let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
    let menu_sender = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = if event.id.0 == STOP {
            TrayAction::StopFocus
        } else {
            TrayAction::Show
        };
        let _ = menu_sender.unbounded_send(action);
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = sender.unbounded_send(TrayAction::Show);
        }
    }));
    receiver
}

/// The stop entry's label: closing is immediate with no session, stopping one waits.
#[cfg(windows)]
fn stop_label(running: bool) -> &'static str {
    if running {
        "Stop Focus…"
    } else {
        "Close Focus"
    }
}
