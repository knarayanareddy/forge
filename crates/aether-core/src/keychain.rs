use std::fs;
use std::path::PathBuf;
use sha2::Digest;

const AUTH_TOKEN_PREFIX: &str = "v2.";
const AUTH_TOKEN_BYTES: usize = 32;
use thiserror::Error;

pub const BYOK_SERVICE: &str = "AetherForge";
pub const BYOK_ACCOUNT: &str = "byok-api-key";
pub const DAEMON_AUTH_ACCOUNT: &str = "daemon-auth-token";

fn gateway_token_account(channel_id: &str) -> String {
    format!("gateway-token-{channel_id}")
}

fn named_secret_account(name: &str) -> String {
    format!("secret-{name}")
}

fn named_secret_env_var(name: &str) -> String {
    format!("AETHER_SECRET_{}", name.to_ascii_uppercase())
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

    pub fn delete(service: &str, account: &str) -> Result<(), KeychainError> {
        store().remove(&(service.to_string(), account.to_string()));
        Ok(())
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

#[cfg(test)]
fn test_keychain_delete(service: &str, account: &str) -> Result<(), KeychainError> {
    test_keychain_backend::delete(service, account)
}

#[cfg(not(test))]
fn security_cli_get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    use std::process::Command;
    let output = Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok(if value.is_empty() { None } else { Some(value) });
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found") || stderr.contains("The specified item could not be found")
    {
        Ok(None)
    } else {
        Err(KeychainError::Access(stderr.into_owned()))
    }
}

#[cfg(not(test))]
fn security_cli_delete(service: &str, account: &str) -> Result<(), KeychainError> {
    use std::process::Command;
    let output = Command::new("security")
        .args(["delete-generic-password", "-s", service, "-a", account])
        .output()
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("could not be found") || stderr.contains("The specified item could not be found")
        {
            Ok(())
        } else {
            Err(KeychainError::Access(stderr.into_owned()))
        }
    }
}

#[cfg(not(test))]
fn platform_keychain_set(service: &str, account: &str, password: &str) -> Result<(), KeychainError> {
    // Never pass secrets in process argv. The apple-native keyring backend calls Security.framework.
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| KeychainError::Access(error.to_string()))?;
    entry
        .set_password(password)
        .map_err(|error| KeychainError::Access(error.to_string()))
}

#[cfg(not(test))]
fn platform_keychain_get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| KeychainError::Access(error.to_string()))?;
    match entry.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => security_cli_get(service, account),
        Err(error) => Err(KeychainError::Access(error.to_string())),
    }
}

#[cfg(not(test))]
fn platform_keychain_delete(service: &str, account: &str) -> Result<(), KeychainError> {
    let _ = security_cli_delete(service, account);
    let entry = keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Access(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
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

fn keychain_delete(service: &str, account: &str) -> Result<(), KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    #[cfg(test)]
    {
        test_keychain_delete(service, account)
    }
    #[cfg(not(test))]
    {
        platform_keychain_delete(service, account)
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

/// Store a brokered secret (Phase 11 slice 11.6 / SEC-01) under a caller-chosen name — a tool
/// authenticates with a secret by name, and only the name ever appears in a `ToolInvocation`,
/// the session log, or the audit log. macOS: Keychain. Non-macOS (CI, this environment's daemon):
/// falls back to a same-named environment variable, matching the existing BYOK/auth-token
/// platform-fallback convention — there is no Keychain to fail closed against off Darwin.
pub fn store_named_secret(name: &str, value: &str) -> Result<(), KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    keychain_set(BYOK_SERVICE, &named_secret_account(name), value)
}

/// Remove a brokered secret from Keychain. macOS only; no-op if absent.
pub fn delete_named_secret(name: &str) -> Result<(), KeychainError> {
    if !cfg!(target_os = "macos") {
        return Err(KeychainError::UnavailableOnPlatform);
    }
    keychain_delete(BYOK_SERVICE, &named_secret_account(name))
}

/// Resolve a brokered secret by name: Keychain on macOS, `AETHER_SECRET_<NAME>` env var
/// elsewhere. Returns `None` if genuinely unset, never a partial/empty value.
pub fn load_named_secret(name: &str) -> Result<Option<String>, KeychainError> {
    if !cfg!(target_os = "macos") {
        return Ok(std::env::var(named_secret_env_var(name))
            .ok()
            .filter(|v| !v.is_empty()));
    }
    keychain_get(BYOK_SERVICE, &named_secret_account(name))
}

/// Generate a versioned 256-bit token using the operating system CSPRNG.
///
/// The version prefix lets startup rotate legacy tokens that were derived from timestamp/PID
/// state rather than trusting them indefinitely.
pub fn secure_random_token() -> Result<String, KeychainError> {
    let mut bytes = [0u8; AUTH_TOKEN_BYTES];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| KeychainError::Access(format!("OS random source failed: {e}")))?;
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("{AUTH_TOKEN_PREFIX}{encoded}"))
}

/// Challenge proof used by the Swift client to authenticate the process answering on the local
/// daemon port. The auth token never crosses the wire during ping; only this nonce-bound digest
/// does. This is a proof-of-possession protocol for the local shared secret, not a replacement for
/// transport encryption.
pub fn daemon_server_proof(token: &str, client_nonce: &str) -> Option<String> {
    if token.is_empty()
        || client_nonce.len() < 32
        || client_nonce.len() > 256
        || !client_nonce.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    const BLOCK: usize = 64;
    let key = token.as_bytes();
    let mut normalized = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = sha2::Sha256::digest(key);
        normalized[..digest.len()].copy_from_slice(&digest);
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK];
    let mut outer_pad = [0x5cu8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }
    let mut inner = sha2::Sha256::new();
    inner.update(inner_pad);
    inner.update(b"aether-daemon-proof-v2\0");
    inner.update(client_nonce.as_bytes());
    let mut outer = sha2::Sha256::new();
    outer.update(outer_pad);
    outer.update(inner.finalize());
    Some(format!("{:x}", outer.finalize()))
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
        if token.starts_with(AUTH_TOKEN_PREFIX) {
            // Keep the manual-client fallback synchronized with Keychain without changing the
            // token. The file writer is atomic and mode-restricted.
            if auth_token_file_enabled() {
                write_daemon_auth_token_file(&token)?;
            }
            return Ok(token);
        }
        // Rotate legacy timestamp/PID-derived tokens on first startup after this upgrade.
    }
    let token = secure_random_token()?;
    store_daemon_auth_token(&token)?;
    Ok(token)
}

/// Store daemon IPC auth token in Keychain and a user-readable fallback file.
pub fn store_daemon_auth_token(token: &str) -> Result<(), KeychainError> {
    keychain_set(BYOK_SERVICE, DAEMON_AUTH_ACCOUNT, token)?;
    if auth_token_file_enabled() {
        write_daemon_auth_token_file(token)?;
    }
    Ok(())
}

fn auth_token_file_enabled() -> bool {
    std::env::var("AETHER_WRITE_AUTH_TOKEN_FILE")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn daemon_auth_token_file() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".aether/daemon_auth_token"))
}

fn write_daemon_auth_token_file(token: &str) -> Result<(), KeychainError> {
    let Some(path) = daemon_auth_token_file() else {
        return Err(KeychainError::Access(
            "HOME is unavailable for daemon auth token fallback".into(),
        ));
    };
    let parent = path
        .parent()
        .ok_or_else(|| KeychainError::Access("daemon auth token path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|e| KeychainError::Access(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|e| KeychainError::Access(e.to_string()))?;
        let tmp = parent.join(format!(".daemon_auth_token.{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = match options.open(&tmp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                fs::remove_file(&tmp).map_err(|e| KeychainError::Access(e.to_string()))?;
                options
                    .open(&tmp)
                    .map_err(|e| KeychainError::Access(e.to_string()))?
            }
            Err(error) => return Err(KeychainError::Access(error.to_string())),
        };
        file.write_all(token.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| KeychainError::Access(e.to_string()))?;
        fs::rename(&tmp, &path).map_err(|e| KeychainError::Access(e.to_string()))?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, token).map_err(|e| KeychainError::Access(e.to_string()))?;
        Ok(())
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

    #[test]
    fn named_secret_falls_back_to_env_var_off_darwin() {
        if cfg!(target_os = "macos") {
            return;
        }
        let name = format!("test-secret-{}", std::process::id());
        assert_eq!(load_named_secret(&name).unwrap(), None);

        std::env::set_var(named_secret_env_var(&name), "shh-its-a-secret");
        let loaded = load_named_secret(&name).unwrap();
        std::env::remove_var(named_secret_env_var(&name));
        assert_eq!(loaded.as_deref(), Some("shh-its-a-secret"));
    }

    #[test]
    fn named_secret_empty_env_var_is_treated_as_unset() {
        if cfg!(target_os = "macos") {
            return;
        }
        let name = format!("test-secret-empty-{}", std::process::id());
        std::env::set_var(named_secret_env_var(&name), "");
        let loaded = load_named_secret(&name).unwrap();
        std::env::remove_var(named_secret_env_var(&name));
        assert_eq!(loaded, None);
    }

    #[test]
    fn store_named_secret_fails_closed_off_darwin() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert!(matches!(
            store_named_secret("whatever", "value"),
            Err(KeychainError::UnavailableOnPlatform)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn named_secret_roundtrip_on_macos() {
        let name = format!("test-secret-{}", std::process::id());
        store_named_secret(&name, "sk-super-secret-value").expect("store on macOS");
        let loaded = load_named_secret(&name).expect("load");
        assert_eq!(loaded.as_deref(), Some("sk-super-secret-value"));
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

    #[test]
    fn secure_tokens_are_versioned_unique_and_full_entropy_length() {
        let first = secure_random_token().unwrap();
        let second = secure_random_token().unwrap();
        assert!(first.starts_with(AUTH_TOKEN_PREFIX));
        assert_eq!(first.len(), AUTH_TOKEN_PREFIX.len() + AUTH_TOKEN_BYTES * 2);
        assert_ne!(first, second);
    }

    #[test]
    fn daemon_proof_is_nonce_bound_and_rejects_invalid_nonce() {
        let token = "v2.test-token";
        let nonce_a = "a".repeat(64);
        let nonce_b = "b".repeat(64);
        let proof_a = daemon_server_proof(token, &nonce_a).unwrap();
        assert_eq!(proof_a.len(), 64);
        assert_ne!(proof_a, daemon_server_proof(token, &nonce_b).unwrap());
        assert!(daemon_server_proof(token, "short").is_none());
    }
}
