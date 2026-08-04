use crate::{load_byok_key, KeychainError, ModelBackend, ModelRouter};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_REGISTRY_REL_PATH: &str = "models/registry.toml";
pub const ENV_REGISTRY_PATH: &str = "AETHER_MODEL_REGISTRY";
pub const ENV_MODEL_PROFILE: &str = "AETHER_MODEL_PROFILE";
pub const ENV_MODEL_PROFILE_COMPLEX: &str = "AETHER_MODEL_PROFILE_COMPLEX";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind { Ollama, #[serde(alias = "openai_compatible")] OpenAiCompatible, Mlx, Gguf }

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ModelProfile {
    pub backend: BackendKind,
    #[serde(default)] pub endpoint: Option<String>,
    #[serde(default)] pub model: Option<String>,
    #[serde(default)] pub base_url: Option<String>,
    #[serde(default)] pub provider: Option<String>,
    #[serde(default)] pub model_path: Option<String>,
    #[serde(default)] pub quant: Option<String>,
    #[serde(default)] pub context_len: Option<u32>,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub hf_repo: Option<String>,
    #[serde(default)] pub hf_file: Option<String>,
    #[serde(default)] pub sha256: Option<String>,
    #[serde(default)] pub role: Option<String>,
}

impl ModelProfile {
    pub fn is_chat_role(&self) -> bool { !matches!(self.role.as_deref(), Some("embed")) }
    pub fn is_inference_ready(&self) -> bool { matches!(self.backend, BackendKind::Ollama | BackendKind::OpenAiCompatible) }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelRegistry {
    pub version: u32,
    pub default_profile: String,
    #[serde(default)] pub default_complex_profile: Option<String>,
    pub profiles: HashMap<String, ModelProfile>,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry IO: {0}")] Io(#[from] std::io::Error),
    #[error("registry parse: {0}")] Parse(#[from] toml::de::Error),
    #[error("profile not found: {0}")] ProfileNotFound(String),
    #[error("registry version {0} unsupported")] UnsupportedVersion(u32),
    #[error("profile {0}: {1}")] InvalidProfile(String, String),
    #[error("BYOK key required for {0}")] MissingByokKey(String),
    #[error("keychain: {0}")] Keychain(#[from] KeychainError),
    #[error("registry not found")] NotFound,
}

impl ModelRegistry {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, RegistryError> { Self::parse(&std::fs::read_to_string(path.as_ref())?) }
    pub fn parse(raw: &str) -> Result<Self, RegistryError> {
        let registry: ModelRegistry = toml::from_str(raw)?;
        if registry.version != 1 { return Err(RegistryError::UnsupportedVersion(registry.version)); }
        if !registry.profiles.contains_key(&registry.default_profile) { return Err(RegistryError::ProfileNotFound(registry.default_profile.clone())); }
        if let Some(ref c) = registry.default_complex_profile { if !registry.profiles.contains_key(c) { return Err(RegistryError::ProfileNotFound(c.clone())); } }
        Ok(registry)
    }
    pub fn profile(&self, id: &str) -> Result<&ModelProfile, RegistryError> { self.profiles.get(id).ok_or_else(|| RegistryError::ProfileNotFound(id.to_string())) }
    pub fn chat_profile_ids(&self) -> Vec<String> { let mut ids: Vec<_> = self.profiles.iter().filter(|(_, p)| p.is_chat_role()).map(|(id, _)| id.clone()).collect(); ids.sort(); ids }
    pub fn resolve_backend(&self, id: &str) -> Result<ModelBackend, RegistryError> { self.profile(id)?.to_backend(id) }
    fn primary_id(&self) -> String {
        std::env::var(ENV_MODEL_PROFILE).ok().filter(|id| self.profiles.contains_key(id)).unwrap_or_else(|| self.default_profile.clone())
    }
    fn complex_id(&self) -> String {
        std::env::var(ENV_MODEL_PROFILE_COMPLEX).ok().filter(|id| self.profiles.contains_key(id)).or_else(|| self.default_complex_profile.clone()).unwrap_or_else(|| self.primary_id())
    }
    pub fn primary_profile_id(&self) -> String { self.primary_id() }
    pub fn build_router(&self) -> Result<ModelRouter, RegistryError> {
        let primary = self.resolve_backend(&self.primary_id())?;
        let cid = self.complex_id();
        let fallback = if cid == self.primary_id() { None } else { Some(self.resolve_backend(&cid)?) };
        Ok(ModelRouter::new(primary, fallback))
    }
}

impl ModelProfile {
    fn to_backend(&self, id: &str) -> Result<ModelBackend, RegistryError> {
        match self.backend {
            BackendKind::Ollama => Ok(ModelBackend::OllamaMlx { endpoint: self.endpoint.clone().unwrap_or_else(|| "http://localhost:11434".into()), model: self.model.clone().ok_or_else(|| RegistryError::InvalidProfile(id.into(), "missing model".into()))? }),
            BackendKind::OpenAiCompatible => {
                let api_key = load_byok_key()?.ok_or_else(|| RegistryError::MissingByokKey(id.into()))?;
                if let Some(u) = &self.base_url { std::env::set_var("AETHER_BYOK_ENDPOINT", u); }
                Ok(ModelBackend::ByokCloud { provider: self.provider.clone().unwrap_or_else(|| "openai".into()), api_key, model: self.model.clone().ok_or_else(|| RegistryError::InvalidProfile(id.into(), "missing model".into()))? })
            }
            BackendKind::Mlx => Ok(ModelBackend::MlxLocal { model_path: self.model_path.clone().ok_or_else(|| RegistryError::InvalidProfile(id.into(), "missing model_path".into()))? }),
            BackendKind::Gguf => Ok(ModelBackend::LlamaCpp { model_path: self.model_path.clone().ok_or_else(|| RegistryError::InvalidProfile(id.into(), "missing model_path".into()))? }),
        }
    }
}

pub fn registry_search_paths() -> Vec<PathBuf> {
    let mut p = Vec::new();
    if let Ok(v) = std::env::var(ENV_REGISTRY_PATH) { p.push(PathBuf::from(v)); }
    if let Ok(cwd) = std::env::current_dir() { p.push(cwd.join(DEFAULT_REGISTRY_REL_PATH)); }
    if let Ok(h) = std::env::var("HOME") { p.push(PathBuf::from(h).join(".aether/registry.toml")); }
    p
}
pub fn discover_registry_path() -> Option<PathBuf> { registry_search_paths().into_iter().find(|p| p.is_file()) }
pub fn load_discovered_registry() -> Result<ModelRegistry, RegistryError> { discover_registry_path().ok_or(RegistryError::NotFound).and_then(|p| ModelRegistry::load_from_path(p)) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_registry() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/registry.toml");
        let registry = ModelRegistry::load_from_path(&path).expect("fixture registry");
        assert_eq!(registry.default_profile, "ollama-local");
        assert!(!registry.chat_profile_ids().contains(&"ollama-embed".to_string()));
    }

    #[test]
    fn mlx_profile_not_inference_ready() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/registry.toml");
        let registry = ModelRegistry::load_from_path(&path).unwrap();
        let mlx = registry.profile("mlx-qwen-3b").unwrap();
        assert_eq!(mlx.backend, BackendKind::Mlx);
        assert!(!mlx.is_inference_ready());
    }
}
