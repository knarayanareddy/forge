use std::fs;
use std::path::PathBuf;
use sha2::Digest;
use thiserror::Error;

pub const BYOK_SERVICE: &str = "AetherForge";
pub const BYOK_ACCOUNT: &str = "byok-api-key";
pub const DAEMON_AUTH_ACCOUNT: &str = "daemon-auth-token";

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
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    let entry = keyring::Entry::new(BYOK_SERVICE, BYOK_ACCOUNT)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    entry
        .set_password(api_key)
        .map_err(|e| KeychainError::Access(e.to_string()))
}

/// Load BYOK API key from Keychain when configured. Returns `None` if unset.
pub fn load_byok_key() -> Result<Option<String>, KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    let entry = keyring::Entry::new(BYOK_SERVICE, BYOK_ACCOUNT)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    match entry.get_password() {
        Ok(key) if !key.is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(KeychainError::Access(e.to_string())),
    }
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
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    let entry = keyring::Entry::new(BYOK_SERVICE, DAEMON_AUTH_ACCOUNT)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => Ok(Some(token)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(KeychainError::Access(e.to_string())),
    }
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
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    let entry = keyring::Entry::new(BYOK_SERVICE, DAEMON_AUTH_ACCOUNT)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    entry
        .set_password(token)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
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
        let token = ensure_daemon_auth_token().expect("ensure token");
        assert!(!token.is_empty());
        let reloaded = load_daemon_auth_token()
            .ok()
            .flatten()
            .or_else(load_daemon_auth_token_file);
        assert_eq!(reloaded.as_deref(), Some(token.as_str()), "keychain reload mismatch");
        assert!(verify_daemon_auth_token_expected(&token, &token));
        assert!(!verify_daemon_auth_token("wrong-token").expect("verify wrong"));
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
    fn byok_store_succeeds_on_macos() {
        let test_key = format!("test-key-{}", std::process::id());
        store_byok_key(&test_key).expect("store on macOS");
    }
}
