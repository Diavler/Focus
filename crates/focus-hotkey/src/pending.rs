//! A stand-in until the XDG GlobalShortcuts portal (build step 9).

use focus_core::Hotkey;
use focus_core::ports::{GlobalHotkey, HotkeyError, HotkeyGuard};

/// Registers nothing and says shortcuts aren't supported yet.
#[derive(Debug, Default)]
pub struct PendingHotkey;

impl GlobalHotkey for PendingHotkey {
    fn listen(
        &self,
        hotkey: Hotkey,
        _pressed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<HotkeyGuard, HotkeyError> {
        if hotkey == Hotkey::Off {
            return Ok(HotkeyGuard::none());
        }
        Err(HotkeyError::Unsupported {
            reason: "the GlobalShortcuts portal arrives in build step 9".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::global_hotkey_contract;

    use super::*;

    #[test]
    fn passes_the_contract() {
        global_hotkey_contract(&PendingHotkey, Hotkey::CtrlAltP);
    }
}
