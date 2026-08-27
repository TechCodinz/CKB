use super::frontier_model_profile::{FrontierModelProfileV2, ModelAvailability, SupportState};
use super::frontier_model_registry::FrontierModelRegistry;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilityRequirements {
    pub min_context_tokens: Option<u64>,
    #[serde(default)] pub required_input_modalities: Vec<String>,
    pub reasoning_mode: Option<String>,
    #[serde(default)] pub require_function_calling: bool,
    #[serde(default)] pub require_structured_output: bool,
    #[serde(default)] pub require_code_execution: bool,
    #[serde(default)] pub require_computer_use: bool,
    #[serde(default)] pub require_mcp: bool,
    #[serde(default)] pub required_named_tools: Vec<String>,
    #[serde(default)] pub require_fresh: bool,
    #[serde(default)] pub allow_preview_capabilities: bool,
    #[serde(default)] pub allow_limited_availability: bool,
    #[serde(default)] pub preference_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectionCandidate {
    pub provider: String,
    pub model: String,
    pub compatible: bool,
    pub reasons: Vec<String>,
    pub verified_at: Option<String>,
    pub stale: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectionResult {
    pub selected: Option<ModelSelectionCandidate>,
    pub compatible: Vec<ModelSelectionCandidate>,
    pub rejected: Vec<ModelSelectionCandidate>,
    pub selection_policy: String,
    pub synthetic: bool,
}

fn support_satisfies(state: &SupportState, allow_preview: bool) -> bool {
    matches!(state, SupportState::Supported)
        || (allow_preview && matches!(state, SupportState::Preview | SupportState::Beta | SupportState::Limited))
}

fn has_named_tool(profile: &FrontierModelProfileV2, tool: &str) -> bool {
    profile.tools.named_tools.iter().any(|value| value.eq_ignore_ascii_case(tool))
}

fn preference_index(requirements: &ModelCapabilityRequirements, profile: &FrontierModelProfileV2) -> usize {
    let exact = format!("{}:{}", profile.provider, profile.model);
    requirements.preference_order.iter()
        .position(|value| value.eq_ignore_ascii_case(&exact) || value.eq_ignore_ascii_case(&profile.model))
        .unwrap_or(usize::MAX)
}

pub fn select_verified_models(
    registry: &FrontierModelRegistry,
    requirements: &ModelCapabilityRequirements,
) -> ModelSelectionResult {
    let now = Utc::now();
    let mut compatible = Vec::new();
    let mut rejected = Vec::new();

    for profile in registry.profiles() {
        let mut reasons = Vec::new();
        if matches!(profile.availability, Some(ModelAvailability::Deprecated | ModelAvailability::Retired)) {
            reasons.push("model lifecycle is deprecated or retired".to_string());
        }
        if matches!(profile.availability, Some(ModelAvailability::Limited)) && !requirements.allow_limited_availability {
            reasons.push("limited availability was not allowed".to_string());
        }
        if let Some(minimum) = requirements.min_context_tokens {
            if profile.context_window_tokens.unwrap_or(0) < minimum {
                reasons.push(format!("context window is below required {minimum} tokens"));
            }
        }
        for modality in &requirements.required_input_modalities {
            if !profile.input_modalities.iter().any(|value| value.eq_ignore_ascii_case(modality)) {
                reasons.push(format!("required input modality `{modality}` is not verified"));
            }
        }
        if let Some(mode) = &requirements.reasoning_mode {
            if !profile.reasoning.modes.iter().any(|value| value.eq_ignore_ascii_case(mode)) {
                reasons.push(format!("reasoning mode `{mode}` is not verified"));
            }
        }
        let allow_preview = requirements.allow_preview_capabilities;
        for (required, state, name) in [
            (requirements.require_function_calling, &profile.tools.function_calling, "function calling"),
            (requirements.require_structured_output, &profile.tools.structured_output, "structured output"),
            (requirements.require_code_execution, &profile.tools.code_execution, "code execution"),
            (requirements.require_computer_use, &profile.tools.computer_use, "computer use"),
            (requirements.require_mcp, &profile.tools.mcp, "MCP"),
        ] {
            if required && !support_satisfies(state, allow_preview) {
                reasons.push(format!("required capability `{name}` is not verified as supported"));
            }
        }
        for tool in &requirements.required_named_tools {
            if !has_named_tool(profile, tool) {
                reasons.push(format!("required named tool `{tool}` is not verified"));
            }
        }
        let stale = profile.is_stale_at(now);
        if requirements.require_fresh && stale == Some(true) {
            reasons.push("profile is stale and requires primary-source re-verification".to_string());
        }

        let candidate = ModelSelectionCandidate {
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            compatible: reasons.is_empty(),
            reasons,
            verified_at: profile.verified_at.clone(),
            stale,
        };
        if candidate.compatible { compatible.push(candidate); } else { rejected.push(candidate); }
    }

    compatible.sort_by(|a, b| {
        let ap = registry.require(&a.provider, &a.model).ok();
        let bp = registry.require(&b.provider, &b.model).ok();
        let ai = ap.map(|p| preference_index(requirements, p)).unwrap_or(usize::MAX);
        let bi = bp.map(|p| preference_index(requirements, p)).unwrap_or(usize::MAX);
        ai.cmp(&bi).then_with(|| a.provider.cmp(&b.provider)).then_with(|| a.model.cmp(&b.model))
    });

    ModelSelectionResult {
        selected: compatible.first().cloned(),
        compatible,
        rejected,
        selection_policy: "verified-capabilities-first; explicit preferenceOrder only; otherwise stable provider/model order; no inferred quality ranking".to_string(),
        synthetic: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_requires_verified_capabilities_and_context() {
        let registry = FrontierModelRegistry::builtin().unwrap();
        let result = select_verified_models(&registry, &ModelCapabilityRequirements {
            min_context_tokens: Some(1_000_000),
            require_structured_output: true,
            require_code_execution: true,
            require_mcp: true,
            preference_order: vec!["openai:gpt-5.6-sol".into()],
            ..Default::default()
        });
        assert_eq!(result.selected.as_ref().map(|c| c.model.as_str()), Some("gpt-5.6-sol"));
        assert!(result.compatible.iter().all(|c| c.compatible));
    }

    #[test]
    fn selector_does_not_treat_unknown_as_supported() {
        let registry = FrontierModelRegistry::builtin().unwrap();
        let result = select_verified_models(&registry, &ModelCapabilityRequirements {
            require_mcp: true,
            preference_order: vec!["anthropic:claude-opus-5".into()],
            ..Default::default()
        });
        assert!(result.rejected.iter().any(|c| c.model == "claude-opus-5"));
    }
}
