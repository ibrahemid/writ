//! Where provider keys are kept.
//!
//! API keys never touch `config.toml`, the database, or disk. They are stored
//! in the platform's credential store under the provider id, or held in memory
//! for the session when no store answers. Nothing here puts a key in a log line
//! or an error string.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use writ_core::ai::providers::{self, ProviderGroup};

use crate::poison::recover_poison;

/// Where a provider key is kept.
///
/// One trait rather than a direct call to the platform store, so the session
/// cache ([`resolve_stored_key`]) is exercised against an in-memory store
/// instead of a machine's real keychain.
pub trait KeyStore: Send + Sync {
    /// The key stored for `account`, or `None` when there is none. `Err` when
    /// the store is unavailable or access was denied.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Stores `key` under `account`, replacing whatever was there.
    fn set(&self, account: &str, key: &str) -> Result<(), String>;
    /// Removes `account`. Removing what is not there succeeds.
    fn delete(&self, account: &str) -> Result<(), String>;
    /// Whether a key written here outlives the process. False for the
    /// in-memory store, which is what the key row's session-only warning is.
    fn is_persistent(&self) -> bool;
}

/// The platform credential store: the macOS Keychain, the Windows Credential
/// Manager, or the Secret Service on Linux (GNOME Keyring, KWallet). A Linux
/// session with no Secret Service answers every call with `Err`, which is what
/// sends a key to the session's memory and the key row to its not-saved line.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
struct OsKeychain;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl KeyStore for OsKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        keychain::get(account)
    }

    fn set(&self, account: &str, key: &str) -> Result<(), String> {
        keychain::set(account, key)
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        keychain::delete(account)
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

/// A store that lives and dies with the process, for a platform with no
/// credential store of its own and for the tests.
#[derive(Default)]
pub struct MemoryKeyStore {
    entries: Mutex<HashMap<String, String>>,
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entries = recover_poison(self.entries.lock(), "writ_ai::keys::MemoryKeyStore::get");
        Ok(entries.get(account).cloned())
    }

    fn set(&self, account: &str, key: &str) -> Result<(), String> {
        let mut entries = recover_poison(self.entries.lock(), "writ_ai::keys::MemoryKeyStore::set");
        entries.insert(account.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let mut entries =
            recover_poison(self.entries.lock(), "writ_ai::keys::MemoryKeyStore::delete");
        entries.remove(account);
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

/// The store this platform keeps keys in.
pub fn platform_key_store() -> Box<dyn KeyStore> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        Box::new(OsKeychain)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Box::new(MemoryKeyStore::default())
    }
}

/// Whether a key is stored for a provider, and whether it is confined to memory
/// for this session (keychain unavailable). Surfaced so the UI can warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AiKeyState {
    /// A key exists for this provider.
    pub is_set: bool,
    /// The key lives only in memory this session; it will be gone on restart.
    pub memory_only: bool,
}

/// Platform store access. Returns `Ok(None)` when no credential exists; `Err`
/// when the store is unavailable or access was denied, which steers callers to
/// the in-memory fallback.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod keychain {
    use keyring::{Entry, Error};

    /// Keychain service name under which provider keys are stored. The account
    /// is the provider id, so switching providers keeps independent keys.
    const KEYCHAIN_SERVICE: &str = "com.writ.ai";

    fn entry(account: &str) -> Result<Entry, String> {
        Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())
    }

    pub fn set(account: &str, key: &str) -> Result<(), String> {
        entry(account)?.set_password(key).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Pure key-state policy: keychain wins; otherwise a memory entry is
/// "set but session-only". Separated from the OS call so it is testable.
pub(crate) fn compute_key_state(
    keychain_hit: bool,
    memory: &HashMap<String, String>,
    account: &str,
) -> AiKeyState {
    if keychain_hit {
        AiKeyState {
            is_set: true,
            memory_only: false,
        }
    } else if memory.contains_key(account) {
        AiKeyState {
            is_set: true,
            memory_only: true,
        }
    } else {
        AiKeyState {
            is_set: false,
            memory_only: false,
        }
    }
}

/// Pure key resolution: keychain value, else the memory value.
pub(crate) fn resolve_key_from(
    keychain_value: Option<String>,
    memory: &HashMap<String, String>,
    account: &str,
) -> Option<String> {
    keychain_value.or_else(|| memory.get(account).cloned())
}

/// Whether a provider's key is ever looked up. A local row never had one, so
/// asking the store for it would raise a password prompt for nothing.
pub(crate) fn keeps_a_key(provider: &str) -> bool {
    !matches!(
        providers::provider(provider).map(|row| row.group),
        Some(ProviderGroup::Local)
    )
}

/// The key stored for a provider, reading the store at most once per session.
///
/// A cached answer, "there is no key" included, is the whole answer: every
/// keychain read on macOS can raise a system password prompt, so a provider
/// with no key must not re-prompt on every rewrite. A local row reads nothing
/// at all. Nothing here, on any path, puts a key in a log line or an error
/// string.
pub(crate) fn resolve_stored_key(
    store: &dyn KeyStore,
    cache: &Mutex<HashMap<String, Option<String>>>,
    provider: &str,
) -> Result<Option<String>, String> {
    {
        let hit = recover_poison(cache.lock(), "writ_ai::keys::resolve_stored_key");
        if let Some(answer) = hit.get(provider) {
            return Ok(answer.clone());
        }
    }

    let remember = |answer: Option<String>| {
        let mut cache = recover_poison(cache.lock(), "writ_ai::keys::resolve_stored_key");
        cache.insert(provider.to_string(), answer.clone());
        answer
    };

    if !keeps_a_key(provider) {
        return Ok(remember(None));
    }
    Ok(remember(store.get(provider)?))
}
