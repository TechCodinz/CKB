//! Runtime registry for verified frontier-model capability profiles.
//!
//! Profiles remain provider metadata only: they may shape request compatibility,
//! context budgeting, and UI warnings, but never upgrade CKB architecture evidence.

use super::frontier_model_profile::{
    adapt_request_for_model, FrontierModelProfileV2, RequestCompatibilityV2,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{error::Error, fmt};

const BUILTIN_PROFILE_JSON: &[&str] = &[
    include_str!("../../../profiles/google/gemini-3.7-flash.json"),
    include_str!("../../../profiles/xai/grok-4.6.json"),
    include_str!("../../../profiles/anthropic/claude-fable-5.json"),
    include_str!("../../../profiles/anthropic/claude-mythos-5.json"),
    include_str!("../../../profiles/anthropic/claude-opus-5.json"),
    include_str!("../../../profiles/anthropic/claude-sonnet-5.json"),
    include_str!("../../../profiles/anthropic/claude-opus-4-8.json"),
];

#[derive(Debug)]
pub enum FrontierModelRegistryError {
    InvalidEmbeddedProfile(serde_json::Error),
    ModelNotFound { provider: String, model: String },
}

impl fmt::Display for FrontierModelRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEmbeddedProfile(error) => {
                write!(f, "invalid embedded frontier-model profile: {error}")
            }
            Self::ModelNotFound { provider, model } => {
                write!(f, "no verified frontier-model profile for {provider}/{model}")
            }
        }
    }
}

impl Error for FrontierModelRegistryError {}

impl From<serde_json::Error> for FrontierModelRegistryError {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidEmbeddedProfile(value)
    }
}

#[derive(Debug, Clone)]
pub struct FrontierModelRegistry {
    profiles: Vec<FrontierModelProfileV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierModelFreshness {
    pub provider: String,
    pub model: String,
    pub verified_at: Option<String>,
    pub stale_after_days: Option<u32>,
    pub stale: Option<bool>,
}

impl FrontierModelRegistry {
    /// Build the registry from source-controlled, primary-source-backed profiles.
    pub fn builtin() -> Result<Self, FrontierModelRegistryError> {
        let profiles = BUILTIN_PROFILE_JSON
            .iter()
            .map(|raw| serde_json::from_str::<FrontierModelProfileV2>(raw))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { profiles })
    }

    pub fn from_profiles(profiles: Vec<FrontierModelProfileV2>) -> Self {
        Self { profiles }
    }

    pub fn profiles(&self) -> &[FrontierModelProfileV2] {
        &self.profiles
    }

    /// Exact model and declared aliases only. No family-prefix guessing is allowed.
    pub fn resolve(&self, provider: &str, model: &str) -> Option<&FrontierModelProfileV2> {
        self.profiles.iter().find(|profile| profile.matches(provider, model))
    }

    pub fn require(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<&FrontierModelProfileV2, FrontierModelRegistryError> {
        self.resolve(provider, model)
            .ok_or_else(|| FrontierModelRegistryError::ModelNotFound {
                provider: provider.to_owned(),
                model: model.to_owned(),
            })
    }

    /// Resolve the verified profile, apply documented parameter transformations,
    /// and reject known-invalid reasoning modes without inventing provider behavior.
    pub fn adapt_request(
        &self,
        provider: &str,
        model: &str,
        request: &Value,
    ) -> Result<RequestCompatibilityV2, FrontierModelRegistryError> {
        let profile = self.require(provider, model)?;
        let mut compatibility = adapt_request_for_model(profile, request);
        validate_reasoning_mode(profile, &compatibility.safe_request, &mut compatibility.errors);
        compatibility.compatible = compatibility.errors.is_empty();
        Ok(compatibility)
    }

    pub fn freshness(&self, now: DateTime<Utc>) -> Vec<FrontierModelFreshness> {
        self.profiles
            .iter()
            .map(|profile| FrontierModelFreshness {
                provider: profile.provider.clone(),
                model: profile.model.clone(),
                verified_at: profile.verified_at.clone(),
                stale_after_days: profile.stale_after_days,
                stale: profile.is_stale_at(now),
            })
            .collect()
    }
}

fn string_at_path<'a>(root: &'a Value, path: &str) -> Option<&'a str> {
    let mut cursor = root;
    for part in path.split('.') {
        cursor = cursor.as_object()?.get(part)?;
    }
    cursor.as_str()
}

fn validate_reasoning_mode(
    profile: &FrontierModelProfileV2,
    request: &Value,
    errors: &mut Vec<String>,
) {
    if profile.reasoning.modes.is_empty() {
        return;
    }

    // Common provider shapes. A profile only constrains a mode when the caller
    // actually supplied one; absence continues to use the provider/model default.
    for path in ["thinking_level", "reasoning.effort", "reasoning_effort"] {
        let Some(mode) = string_at_path(request, path) else { continue };
        if !profile
            .reasoning
            .modes
            .iter()
            .any(|supported| supported.eq_ignore_ascii_case(mode))
        {
            errors.push(format!(
                "`{path}={mode}` is incompatible with {}/{}; verified modes: {}",
                profile.provider,
                profile.model,
                profile.reasoning.modes.join(", ")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_registry_resolves_new_exact_models() {
        let registry = FrontierModelRegistry::builtin().expect("embedded profiles must parse");

        let gemini = registry
            .require("GOOGLE", "gemini-3.7-flash")
            .expect("Gemini 3.7 Flash must resolve");
        assert_eq!(gemini.reasoning.modes, vec!["low", "medium", "high"]);
        assert_eq!(gemini.reasoning.default_mode.as_deref(), Some("medium"));

        let grok = registry
            .require("xai", "grok-4.6")
            .expect("Grok 4.6 must resolve");
        assert!(grok.reasoning.modes.iter().any(|mode| mode == "xhigh"));

        let opus = registry
            .require("ANTHROPIC", "claude-opus-5")
            .expect("Claude Opus 5 must resolve");
        assert_eq!(opus.context_window_tokens, Some(1_000_000));
        assert_eq!(opus.max_output_tokens, Some(128_000));
        assert!(opus.reasoning.modes.iter().any(|mode| mode == "max"));

        let mythos = registry
            .require("anthropic", "claude-mythos-5")
            .expect("Claude Mythos 5 must resolve");
        assert_eq!(mythos.availability, Some(super::super::frontier_model_profile::ModelAvailability::Limited));
    }

    #[test]
    fn gemini_minimal_reasoning_is_rejected_by_runtime_adapter() {
        let registry = FrontierModelRegistry::builtin().expect("embedded profiles must parse");
        let result = registry
            .adapt_request(
                "google",
                "gemini-3.7-flash",
                &json!({"thinking_level": "minimal", "input": "inspect this change"}),
            )
            .expect("profile must exist");

        assert!(!result.compatible);
        assert!(result.errors.iter().any(|error| error.contains("minimal")));
    }

    #[test]
    fn grok_rejected_parameters_remain_hard_errors() {
        let registry = FrontierModelRegistry::builtin().expect("embedded profiles must parse");
        let result = registry
            .adapt_request(
                "xai",
                "grok-4.6",
                &json!({"input": "x", "stop": ["END"], "reasoning": {"effort": "xhigh"}}),
            )
            .expect("profile must exist");

        assert!(!result.compatible);
        assert!(result.errors.iter().any(|error| error.contains("stop")));
        assert!(!result.errors.iter().any(|error| error.contains("xhigh")));
    }

    #[test]
    fn anthropic_effort_modes_are_validated_without_family_guessing() {
        let registry = FrontierModelRegistry::builtin().expect("embedded profiles must parse");
        let result = registry
            .adapt_request(
                "anthropic",
                "claude-sonnet-5",
                &json!({"input": "x", "reasoning": {"effort": "ultra"}}),
            )
            .expect("profile must exist");
        assert!(!result.compatible);
        assert!(result.errors.iter().any(|error| error.contains("ultra")));
    }

    #[test]
    fn unknown_models_do_not_fall_back_to_family_guesses() {
        let registry = FrontierModelRegistry::builtin().expect("embedded profiles must parse");
        assert!(registry.resolve("google", "gemini-3.7-pro").is_none());
        assert!(registry.resolve("anthropic", "claude-opus-5.1").is_none());
    }
}
