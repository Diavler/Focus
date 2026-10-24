//! The `Secrets` port backed by the OS keychain: Windows Credential Manager, for the current user
//! only and without admin rights.
//!
//! Linux comes in build step 9 (the Secret Service over D-Bus). Until then storing fails with a
//! reason and nothing is ever found, so the integrations report themselves as not set up.

use focus_core::ports::{SecretKey, Secrets, SecretsError};

/// Keeps credentials in the OS keychain under one service name, one entry per [`SecretKey`].
///
/// Opening the keychain can't stop Focus from starting: if it fails, every read and write
/// reports why, and only the integrations that need it show a problem.
#[derive(Debug)]
pub struct KeyringSecrets {
    service: String,
    #[cfg(windows)]
    store: Result<std::sync::Arc<windows_native_keyring_store::Store>, String>,
}

impl KeyringSecrets {
    /// The service name Focus's credentials are stored under.
    pub const SERVICE: &str = "Focus";

    /// The keychain entries Focus uses.
    pub fn new() -> Self {
        Self::with_service(Self::SERVICE)
    }

    /// Entries under another service name, so tests never touch the real credentials.
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
            #[cfg(windows)]
            store: windows_native_keyring_store::Store::new().map_err(|error| error.to_string()),
        }
    }
}

impl Default for KeyringSecrets {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl Secrets for KeyringSecrets {
    fn load(&self, key: SecretKey) -> Result<Option<String>, SecretsError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(unavailable(error)),
        }
    }

    fn store(&self, key: SecretKey, value: Option<&str>) -> Result<(), SecretsError> {
        let entry = self.entry(key)?;
        let result = match value {
            Some(value) => entry.set_password(value),
            None => entry.delete_credential(),
        };
        match result {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(unavailable(error)),
        }
    }
}

#[cfg(windows)]
impl KeyringSecrets {
    fn entry(&self, key: SecretKey) -> Result<keyring_core::Entry, SecretsError> {
        use keyring_core::api::CredentialStoreApi;
        let store = self
            .store
            .as_ref()
            .map_err(|reason| SecretsError::Unavailable {
                reason: reason.clone(),
            })?;
        store
            .build(&self.service, key.name(), None)
            .map_err(unavailable)
    }
}

#[cfg(windows)]
fn unavailable(error: keyring_core::Error) -> SecretsError {
    SecretsError::Unavailable {
        reason: error.to_string(),
    }
}

#[cfg(not(windows))]
impl Secrets for KeyringSecrets {
    fn load(&self, _key: SecretKey) -> Result<Option<String>, SecretsError> {
        Ok(None)
    }

    fn store(&self, _key: SecretKey, value: Option<&str>) -> Result<(), SecretsError> {
        match value {
            None => Ok(()),
            Some(_) => Err(SecretsError::Unavailable {
                reason: format!(
                    "storing credentials on Linux arrives in build step 9 (service {:?})",
                    self.service
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use focus_core::testing::secrets_contract;

    use super::*;

    /// Writes (and then removes) entries under a test-only service name in the real keychain,
    /// which a CI service account may not have, so it runs on request:
    /// `cargo test -p secrets-keyring -- --ignored`.
    #[test]
    #[ignore = "uses the real OS keychain"]
    fn passes_the_contract_in_the_real_keychain() {
        let secrets = KeyringSecrets::with_service("Focus contract test");
        secrets_contract(&secrets);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_linux_placeholder_passes_the_contract() {
        secrets_contract(&KeyringSecrets::new());
    }
}
