use std::fs;
use std::path::PathBuf;
use sha2::Digest;
use thiserror::Error;

pub const BYOK_SERVICE: &str = "AetherForge";
pub const BYOK_ACCOUNT: &str = "byok-api-key";
pub const DAEMON_AUTH_ACCOUNT: &str = "daemon-auth-token";

fn gateway_token_account(channel_id: &str) -> String {
    format!("gateway-token-{channel_id}")
}

#[cfg(test)]
mod test_keychain_backend {
    use super::KeychainError;
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    fn store() -> std::sync::MutexGuard<'static, HashMap<(String, String), String>> {
        static MOCK: LazyLock<Mutex<HashMap<(String, String), String>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        MOCK.lock()
            .map_err(|e| KeychainError::Access(e.to_string()))
            .expect("mock keychain lock")
    }

    pub fn set(service: &str, account: &str, password: &str) -> Result<(), KeychainError> {
        store().insert(
            (service.to_string(), account.to_string()),
            password.to_string(),
        );
        Ok(())
    }

    pub fn get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        Ok(store()
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }
}

#[cfg(test)]
fn test_keychain_set(service: &str, account: &str, password: &str) -> Result<(), KeychainError> {
    test_keychain_backend::set(service, account, password)
}

#[cfg(test)]
fn test_keychain_get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    test_keychain_backend::get(service, account)
}

#[cfg(not(test))]
fn platform_keychain_set(service: &str, account: &str, password: &str) -> Result<(), KeychainError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    entry
        .set_password(password)
        .map_err(|e| KeychainError::Access(e.to_string()))
}

#[cfg(not(test))]
fn platform_keychain_get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    match entry.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(KeychainError::Access(e.to_string())),
    }
}

fn keychain_set(service: &str, account: &str, password: &str) -> Result<(), KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    #[cfg(test)]
    {
        test_keychain_set(service, account, password)
    }
    #[cfg(not(test))]
    {
        platform_keychain_set(service, account, password)
    }
}

fn keychain_get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    #[cfg(test)]
    {
        test_keychain_get(service, account)
    }
    #[cfg(not(test))]
    {
        platform_keychain_get(service, account)
    }
}

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("BYOK Keychain storage is only available on macOS")]
    UnavailableOnPlatform,
    #[error("Keychain access failed: {0}")]
    Access(String),
    #[error("BYOK API key not found in Keychain (service={0}, account={1})")]
    NotFound(&'static str, &'static str),
}

/// Store a BYOK API key in the macOS Keychain. Fail-closed on non-macOS.
pub fn store_byok_key(api_key: &str) -> Result<(), KeychainError> {
    keychain_set(BYOK_SERVICE, BYOK_ACCOUNT, api_key)
}

/// Load BYOK API key from Keychain when configured. Returns `None` if unset.
pub fn load_byok_key() -> Result<Option<String>, KeychainError> {
    keychain_get(BYOK_SERVICE, BYOK_ACCOUNT)
}

/// Store a gateway channel token in Keychain (Slack/Telegram/Discord). macOS only.
pub fn store_gateway_token(channel_id: &str, token: &str) -> Result<(), KeychainError> {
    keychain_set(BYOK_SERVICE, &gateway_token_account(channel_id), token)
}

/// Load gateway channel token from Keychain. Returns `None` if unset.
pub fn load_gateway_token(channel_id: &str) -> Result<Option<String>, KeychainError> {
    keychain_get(BYOK_SERVICE, &gateway_token_account(channel_id))
}

fn random_auth_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = sha2::Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    format!("{:x}", hasher.finalize())
}

/// Load daemon IPC auth token from Keychain. Returns `None` if unset.
pub fn load_daemon_auth_token() -> Result<Option<String>, KeychainError> {
    keychain_get(BYOK_SERVICE, DAEMON_AUTH_ACCOUNT)
}

/// Ensure a daemon auth token exists in Keychain (load or generate). macOS only.
pub fn ensure_daemon_auth_token() -> Result<String, KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    if let Some(token) = load_daemon_auth_token()? {
        return Ok(token);
    }
    let token = random_auth_token();
    store_daemon_auth_token(&token)?;
    Ok(token)
}

/// Store daemon IPC auth token in Keychain and a user-readable fallback file.
pub fn store_daemon_auth_token(token: &str) -> Result<(), KeychainError> {
    keychain_set(BYOK_SERVICE, DAEMON_AUTH_ACCOUNT, token)?;
    write_daemon_auth_token_file(token);
    Ok(())
}

fn daemon_auth_token_file() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".aether/daemon_auth_token"))
}

fn write_daemon_auth_token_file(token: &str) {
    let Some(path) = daemon_auth_token_file() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write(&path, token).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
}

pub fn load_daemon_auth_token_file() -> Option<String> {
    let path = daemon_auth_token_file()?;
    let token = fs::read_to_string(path).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn tokens_match(provided: &str, expected: &str) -> bool {
    if provided.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in provided.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Constant-time-ish compare for daemon auth tokens.
pub fn verify_daemon_auth_token(provided: &str) -> Result<bool, KeychainError> {
    if !cfg!(target_os = "macos") {
        if let Ok(expected) = std::env::var("AETHER_DAEMON_AUTH_TOKEN") {
            if expected.is_empty() {
                return Ok(true);
            }
            return Ok(tokens_match(provided, &expected));
        }
        return Ok(true);
    }

    let expected = match load_daemon_auth_token()? {
        Some(token) => token,
        None => load_daemon_auth_token_file().unwrap_or_default(),
    };
    if expected.is_empty() {
        return Ok(false);
    }
    Ok(tokens_match(provided, &expected))
}

/// Verify against an in-memory token (daemon startup) with optional reload fallback.
pub fn verify_daemon_auth_token_expected(provided: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    tokens_match(provided, expected)
}

/// Require a BYOK key when `AETHER_BYOK_PROVIDER` is set. Fail-closed off Darwin.
pub fn require_byok_key_if_configured() -> Result<Option<String>, KeychainError> {
    let provider = match std::env::var("AETHER_BYOK_PROVIDER") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => return Ok(None),
    };

    if !cfg!(target_os = "macos") {
        let _ = provider;
        return Err(KeychainError::UnavailableOnPlatform);
    }

    match load_byok_key()? {
        Some(key) => Ok(Some(key)),
        None => Err(KeychainError::NotFound(BYOK_SERVICE, BYOK_ACCOUNT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byok_fail_closed_off_darwin_when_provider_set() {
        if cfg!(target_os = "macos") {
            return;
        }
        std::env::set_var("AETHER_BYOK_PROVIDER", "openai");
        let result = require_byok_key_if_configured();
        std::env::remove_var("AETHER_BYOK_PROVIDER");
        assert!(matches!(result, Err(KeychainError::UnavailableOnPlatform)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn daemon_auth_roundtrip() {
        let token = format!("daemon-test-{}", std::process::id());
        store_daemon_auth_token(&token).expect("store token");
        let reloaded = load_daemon_auth_token()
            .expect("load")
            .or_else(load_daemon_auth_token_file);
        assert_eq!(reloaded.as_deref(), Some(token.as_str()), "keychain reload mismatch");
        assert!(verify_daemon_auth_token_expected(&token, &token));
        assert!(!verify_daemon_auth_token_expected("wrong-token", &token));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn byok_store_load_roundtrip_on_macos() {
        let test_key = format!("test-key-{}", std::process::id());
        store_byok_key(&test_key).expect("store on macOS");
        let loaded = load_byok_key().expect("load");
        assert_eq!(loaded.as_deref(), Some(test_key.as_str()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn gateway_token_roundtrip_on_macos() {
        let channel_id = format!("gate-{}", std::process::id());
        let token = format!("xoxb-test-{}", std::process::id());
        store_gateway_token(&channel_id, &token).expect("store gateway token");
        let loaded = load_gateway_token(&channel_id).expect("load gateway token");
        assert_eq!(loaded.as_deref(), Some(token.as_str()));
    }
}
