//! The parking lot: stray thoughts and "later" links. Kept in the saved state, so parking works
//! offline and without an account, and sent to the task inbox once one is set up and reachable.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::time::Timestamp;

/// The most items the parking lot keeps.
pub const MAX_PARKED: usize = 500;

/// The longest text or link an item may have, in characters.
pub const MAX_PARKED_TEXT: usize = 2000;

/// One parked thought or link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkedItem {
    /// Unique within this parking lot.
    pub id: u64,
    /// What was typed, or the page's title.
    pub text: String,
    /// The page, for a link saved with Later.
    pub url: Option<String>,
    /// When it was parked.
    pub parked_at: Timestamp,
    /// When it reached the task inbox, if it has.
    pub sent_at: Option<Timestamp>,
    /// Why the task inbox refused it, if it did; it then stays here and isn't sent again.
    #[serde(default)]
    pub refused: Option<String>,
}

impl ParkedItem {
    /// Whether it still has to go to the inbox: not sent, and not refused.
    pub fn waiting(&self) -> bool {
        self.sent_at.is_none() && self.refused.is_none()
    }

    /// Names this item to the inbox, so a send repeated after a lost answer isn't added twice.
    pub fn request_id(&self) -> String {
        format!("focus-parked-{}-{}", self.parked_at.as_millis(), self.id)
    }
}

/// Why something couldn't be parked.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParkError {
    /// Nothing to park.
    #[error("type something to park")]
    Empty,
    /// The text or link is too long.
    #[error("that's too long to park (at most {max} characters)")]
    TooLong {
        /// The most characters allowed.
        max: usize,
    },
    /// The parking lot is full.
    #[error("the parking lot is full ({max} items); clear some first")]
    Full {
        /// The most items kept.
        max: usize,
    },
}

/// Everything parked, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkingLot {
    items: Vec<ParkedItem>,
    next_id: u64,
    /// Goes up with every change, so a window can tell its copy is out of date.
    #[serde(default)]
    revision: u64,
}

impl ParkingLot {
    /// Every item, oldest first.
    pub fn items(&self) -> &[ParkedItem] {
        &self.items
    }

    /// Goes up with every change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many items still have to go to the inbox.
    pub fn waiting(&self) -> usize {
        self.items.iter().filter(|item| item.waiting()).count()
    }

    /// How many items the inbox refused.
    pub fn refused(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.refused.is_some())
            .count()
    }

    /// The items that still have to go to the inbox, oldest first.
    pub fn unsent(&self) -> Vec<ParkedItem> {
        self.items
            .iter()
            .filter(|item| item.waiting())
            .cloned()
            .collect()
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Parks `text` (or, if it's blank, the link itself) at `now`. Returns the new item's id.
    pub fn park(
        &mut self,
        text: &str,
        url: Option<&str>,
        now: Timestamp,
    ) -> Result<u64, ParkError> {
        let url = url.map(str::trim).filter(|url| !url.is_empty());
        let text = match text.trim() {
            "" => url.ok_or(ParkError::Empty)?,
            text => text,
        };
        let too_long = |value: &str| value.chars().count() > MAX_PARKED_TEXT;
        if too_long(text) || url.is_some_and(too_long) {
            return Err(ParkError::TooLong {
                max: MAX_PARKED_TEXT,
            });
        }
        if self.items.len() >= MAX_PARKED {
            return Err(ParkError::Full { max: MAX_PARKED });
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.items.push(ParkedItem {
            id,
            text: text.to_owned(),
            url: url.map(str::to_owned),
            parked_at: now,
            sent_at: None,
            refused: None,
        });
        self.changed();
        Ok(id)
    }

    /// Records that item `id` reached the inbox at `now`. False if there's no such item.
    pub fn mark_sent(&mut self, id: u64, now: Timestamp) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.sent_at.get_or_insert(now);
        self.changed();
        true
    }

    /// Records that the inbox refused item `id`, so it isn't sent again. False if there's no
    /// such item.
    pub fn mark_refused(&mut self, id: u64, reason: &str) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.refused = Some(reason.to_owned());
        self.changed();
        true
    }

    /// Removes item `id`. False if there's no such item.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        let removed = self.items.len() != before;
        if removed {
            self.changed();
        }
        removed
    }

    /// Removes every item that reached the inbox, returning how many.
    pub fn remove_sent(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|item| item.sent_at.is_none());
        let removed = before - self.items.len();
        if removed > 0 {
            self.changed();
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_millis(1_000);

    #[test]
    fn parks_text_or_a_bare_link() {
        let mut lot = ParkingLot::default();
        let thought = lot.park("  look up serde flatten ", None, NOW).unwrap();
        let link = lot.park("", Some(" https://docs.rs/serde "), NOW).unwrap();
        assert_ne!(thought, link);
        let items = lot.items();
        assert_eq!(items[0].text, "look up serde flatten");
        assert_eq!(
            items[1].text, "https://docs.rs/serde",
            "a bare link is its own text"
        );
        assert_eq!(items[1].url.as_deref(), Some("https://docs.rs/serde"));
        assert_eq!(lot.waiting(), 2);
        assert_eq!(lot.park(" ", Some(" "), NOW), Err(ParkError::Empty));
    }

    #[test]
    fn refuses_what_is_too_long_or_too_many() {
        let mut lot = ParkingLot::default();
        let long = "x".repeat(MAX_PARKED_TEXT + 1);
        assert_eq!(
            lot.park(&long, None, NOW),
            Err(ParkError::TooLong {
                max: MAX_PARKED_TEXT
            })
        );
        assert!(lot.park("ok", Some(&long), NOW).is_err());
        for n in 0..MAX_PARKED {
            lot.park(&n.to_string(), None, NOW).unwrap();
        }
        assert_eq!(
            lot.park("one more", None, NOW),
            Err(ParkError::Full { max: MAX_PARKED })
        );
    }

    #[test]
    fn tracks_what_was_sent_and_clears_it() {
        let mut lot = ParkingLot::default();
        let first = lot.park("a", None, NOW).unwrap();
        let second = lot.park("b", None, NOW).unwrap();
        assert!(lot.mark_sent(first, Timestamp::from_millis(2_000)));
        assert!(!lot.mark_sent(99, NOW));
        assert_eq!(
            lot.unsent().iter().map(|i| i.id).collect::<Vec<_>>(),
            [second]
        );
        assert_eq!(lot.remove_sent(), 1);
        assert!(lot.remove(second));
        assert!(!lot.remove(second));
        assert!(lot.items().is_empty());
        let third = lot.park("c", None, NOW).unwrap();
        assert!(third > second, "ids aren't reused");
    }

    #[test]
    fn refused_items_stay_but_are_not_sent_again() {
        let mut lot = ParkingLot::default();
        let first = lot.park("too long for the inbox", None, NOW).unwrap();
        let second = lot.park("fine", None, NOW).unwrap();
        let before = lot.revision();
        assert!(lot.mark_refused(first, "HTTP 400"));
        assert!(lot.revision() > before, "a change bumps the revision");
        assert_eq!(
            lot.unsent().iter().map(|i| i.id).collect::<Vec<_>>(),
            [second]
        );
        assert_eq!((lot.waiting(), lot.refused()), (1, 1));
        assert!(!lot.mark_refused(99, "x"));
        let unchanged = lot.revision();
        assert!(!lot.remove(99));
        assert_eq!(lot.revision(), unchanged, "no change, same revision");
    }

    #[test]
    fn request_ids_differ_per_item() {
        let mut lot = ParkingLot::default();
        lot.park("a", None, NOW).unwrap();
        lot.park("b", None, NOW).unwrap();
        assert_ne!(lot.items()[0].request_id(), lot.items()[1].request_id());
    }
}
