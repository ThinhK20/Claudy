use keyring::{self, Entry};

/// Windows Credential Manager entry name: "Claudy — AI provider API keys".
const SERVICE: &str = "com.claudy.app";

fn entry(provider_id: &str) -> Result<Entry, String> {
    if !crate::config::PROVIDER_IDS.contains(&provider_id) {
        return Err(format!("Unknown AI provider \"{provider_id}\""));
    }
    Entry::new(SERVICE, &format!("provider:{provider_id}"))
        .map_err(|e| format!("Credential store unavailable: {e}"))
}

/// Store an API key. An empty/whitespace key means "remove it" — the UI's
/// clear action and save-empty-field collapse to one path, and no empty
/// strings ever land in the credential store.
pub fn set(provider_id: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return delete(provider_id);
    }
    entry(provider_id)?
        .set_password(key)
        .map_err(|e| format!("Could not store API key: {e}"))
}

/// `None` = no key stored (keyring `NoEntry` is not an error for us).
pub fn get(provider_id: &str) -> Result<Option<String>, String> {
    match entry(provider_id)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Could not read API key: {e}")),
    }
}

pub fn delete(provider_id: &str) -> Result<(), String> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not delete API key: {e}")),
    }
}

/// The key itself is NEVER returned to the webview — only whether one exists.
#[tauri::command]
pub fn has_api_key(provider: String) -> Result<bool, String> {
    Ok(get(&provider)?.is_some())
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<(), String> {
    set(&provider, &key)
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), String> {
    delete(&provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring::credential::{
        Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence,
    };
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::{Mutex, Once, OnceLock};

    // keyring v3's built-in `keyring::mock` store does NOT share state across
    // `Entry` instances: `MockCredentialBuilder::build()` returns a brand-new
    // `MockCredential::default()` every call (verified by reading
    // keyring-3.6.3/src/mock.rs), and its `persistence()` is `EntryOnly` — the
    // doc literally says "this keystore keeps the password in the entry!". Since
    // `secrets::entry()` constructs a fresh `Entry` per call, a `set()` followed
    // by a separate `get()` call always lands on a different, empty
    // `MockCredential` and reports `NoEntry`. A probe test confirmed this: two
    // `Entry::new("svc","user")` instances under the mock builder do not see
    // each other's `set_password`. So we install a tiny fake store here instead,
    // backed by a process-global map keyed by (service, user) — like a real OS
    // credential store — so `set()`/`get()`/`delete()` genuinely round-trip.
    struct FakeCredential {
        key: (String, String),
    }

    fn store() -> &'static Mutex<HashMap<(String, String), String>> {
        static STORE: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    impl CredentialApi for FakeCredential {
        fn set_password(&self, password: &str) -> keyring::Result<()> {
            store()
                .lock()
                .unwrap()
                .insert(self.key.clone(), password.to_string());
            Ok(())
        }

        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            self.set_password(&String::from_utf8_lossy(secret))
        }

        fn get_password(&self) -> keyring::Result<String> {
            store()
                .lock()
                .unwrap()
                .get(&self.key)
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            self.get_password().map(String::into_bytes)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            store()
                .lock()
                .unwrap()
                .remove(&self.key)
                .map(|_| ())
                .ok_or(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FakeCredentialBuilder;

    impl CredentialBuilderApi for FakeCredentialBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<Credential>> {
            Ok(Box::new(FakeCredential {
                key: (service.to_string(), user.to_string()),
            }))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn persistence(&self) -> CredentialPersistence {
            CredentialPersistence::UntilDelete
        }
    }

    /// Install the fake store exactly once process-wide. Tests run in parallel
    /// threads by default; re-installing a builder per-test would be harmless
    /// here (the fake store's state lives in a separate static, not in the
    /// builder), but `Once` keeps the intent explicit and matches how a
    /// real app installs its credential builder a single time at startup.
    /// Tests use DISTINCT provider ids to stay isolated under parallel execution.
    fn use_mock_store() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            keyring::set_default_credential_builder(Box::new(FakeCredentialBuilder));
        });
    }

    #[test]
    fn set_then_get_round_trips() {
        use_mock_store();
        set("ollama", "sk-test-123").unwrap();
        assert_eq!(get("ollama").unwrap(), Some("sk-test-123".into()));
    }

    #[test]
    fn get_without_a_stored_key_is_none_not_an_error() {
        use_mock_store();
        assert_eq!(get("gemini").unwrap(), None);
    }

    #[test]
    fn empty_key_deletes_and_delete_is_idempotent() {
        use_mock_store();
        set("anthropic", "sk-x").unwrap();
        set("anthropic", "   ").unwrap(); // whitespace-only = delete
        assert_eq!(get("anthropic").unwrap(), None);
        delete("anthropic").unwrap(); // nothing stored -> still Ok
    }

    #[test]
    fn unknown_provider_is_rejected_before_touching_the_store() {
        use_mock_store();
        let err = set("skynet", "k").unwrap_err();
        assert!(err.contains("skynet"), "got: {err}");
        assert!(get("skynet").is_err());
        assert!(delete("skynet").is_err());
    }
}
