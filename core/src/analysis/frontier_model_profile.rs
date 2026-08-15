//! Vendor-neutral model capability/lifecycle metadata for fast-moving frontier APIs.
//!
//! This module is intentionally separated from architecture evidence truth.
//! Provider/model metadata can influence transport/request compatibility and
//! context budgeting, but it can never upgrade STATIC/PREDICTED evidence into
//! RUNTIME/VALIDATION facts or act as an unobserved quality score.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SupportState {
    Supported,
    Unsupported,
    Preview,
    Beta,
    Limited,
    Unknown,
}

impl Default for SupportState {
    fn default() -> Self { Self::Unknown }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAvailability {
    Ga,
    Preview,
    Limited,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderLifecycleState {
    Active,
    Legacy,
    Deprecated,
    Retired,
    Preview,
    ShutdownScheduled,
    Shutdown,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleDateKind {
    Exact,
    Tentative,
    Earliest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleEvidenceSource {
    pub kind: String,
    pub reference: String,
    pub observed_at: String,
}

/// Provider lifecycle evidence is separate from capability evidence. A model
/// may be known to be deprecated/retired while its tool/context surface remains
/// unknown. Calendar dates do not promote provider_state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontierModelLifecycleRecord {
    pub provider: String,
    pub model: String,
    pub provider_state: ProviderLifecycleState,
    pub deprecated_at: Option<String>,
    pub retirement_at: Option<String>,
    pub retirement_date_kind: Option<LifecycleDateKind>,
    pub recommended_replacement: Option<String>,
    pub verified_at: String,
    pub source: LifecycleEvidenceSource,
    pub note: String,
    pub synthetic: bool,
}

impl FrontierModelLifecycleRecord {
    pub fn migration_required(&self) -> bool {
        matches!(
            self.provider_state,
            ProviderLifecycleState::Deprecated
                | ProviderLifecycleState::Retired
                | ProviderLifecycleState::ShutdownScheduled
                | ProviderLifecycleState::Shutdown
        )
    }

    pub fn execution_eligible(&self) -> bool {
        !matches!(
            self.provider_state,
            ProviderLifecycleState::Deprecated
                | ProviderLifecycleState::Retired
                | ProviderLifecycleState::ShutdownScheduled
                | ProviderLifecycleState::Shutdown
        )
    }

    /// Reports date relation for migration urgency/UI only. Callers must never
    /// infer Retired/Shutdown solely because a date has passed.
    pub fn retirement_date_has_passed(&self, now: chrono::DateTime<chrono::Utc>) -> Option<bool> {
        let value = self.retirement_at.as_ref()?;
        let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?.with_timezone(&chrono::Utc);
        Some(now >= parsed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningCapabilityV2 {
    #[serde(default)]
    pub support: SupportState,
    #[serde(default)]
    pub modes: Vec<String>,
    pub default_mode: Option<String>,
    #[serde(default)]
    pub adaptive: SupportState,
    pub always_on: Option<bool>,
    pub manual_budget_supported: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapabilityV2 {
    #[serde(default)] pub function_calling: SupportState,
    #[serde(default)] pub structured_output: SupportState,
    #[serde(default)] pub parallel_function_calling: SupportState,
    #[serde(default)] pub code_execution: SupportState,
    #[serde(default)] pub computer_use: SupportState,
    #[serde(default)] pub mcp: SupportState,
    #[serde(default)] pub named_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterMigrationV2 {
    pub from: String,
    pub to: Option<String>,
    pub action: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RequestConstraintsV2 {
    #[serde(default)] pub deprecated_parameters: Vec<String>,
    #[serde(default)] pub rejected_parameters: Vec<String>,
    #[serde(default)] pub ignored_parameters: Vec<String>,
    #[serde(default)] pub unsupported_parameters: Vec<String>,
    #[serde(default)] pub unsupported_turn_patterns: Vec<String>,
    #[serde(default)] pub parameter_migrations: Vec<ParameterMigrationV2>,
    #[serde(default)] pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySourceV2 {
    pub kind: String,
    pub reference: String,
    pub observed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontierModelProfileV2 {
    pub provider: String,
    pub model: String,
    #[serde(default)] pub aliases: Vec<String>,
    pub availability: Option<ModelAvailability>,
    pub released_at: Option<String>,
    pub verified_at: Option<String>,
    pub stale_after_days: Option<u32>,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub knowledge_cutoff: Option<String>,
    #[serde(default)] pub input_modalities: Vec<String>,
    #[serde(default)] pub output_modalities: Vec<String>,
    #[serde(default)] pub api_surfaces: Vec<String>,
    pub preferred_api_surface: Option<String>,
    #[serde(default)] pub reasoning: ReasoningCapabilityV2,
    #[serde(default)] pub tools: ToolCapabilityV2,
    #[serde(default)] pub request_constraints: RequestConstraintsV2,
    #[serde(default)] pub tokenizer_notes: Vec<String>,
    #[serde(default)] pub declared_sources: Vec<CapabilitySourceV2>,
    pub source_kind: Option<String>,
}

impl FrontierModelProfileV2 {
    pub fn matches(&self, provider: &str, model: &str) -> bool {
        self.provider.eq_ignore_ascii_case(provider)
            && (self.model.eq_ignore_ascii_case(model)
                || self.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(model)))
    }

    /// Freshness is advisory metadata only. Staleness means "re-verify provider
    /// docs", not "model unsupported" and never changes architecture evidence.
    pub fn is_stale_at(&self, now: chrono::DateTime<chrono::Utc>) -> Option<bool> {
        let verified = self.verified_at.as_ref()?;
        let days = self.stale_after_days? as i64;
        let parsed = chrono::DateTime::parse_from_rfc3339(verified).ok()?.with_timezone(&chrono::Utc);
        Some(now.signed_duration_since(parsed) > chrono::Duration::days(days))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestCompatibilityV2 {
    pub provider: String,
    pub model: String,
    pub preferred_api_surface: Option<String>,
    pub safe_request: Value,
    pub removed_parameters: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub compatible: bool,
    pub source_kind: Option<String>,
    pub synthetic: bool,
}

fn has_path(root: &Value, path: &str) -> bool {
    let mut cursor = root;
    for part in path.split('.') {
        let Some(next) = cursor.as_object().and_then(|object| object.get(part)) else { return false; };
        cursor = next;
    }
    true
}

fn delete_path(root: &mut Value, path: &str) -> bool {
    let parts = path.split('.').collect::<Vec<_>>();
    let Some((last, parents)) = parts.split_last() else { return false; };
    let mut cursor = root;
    for part in parents {
        let Some(next) = cursor.as_object_mut().and_then(|object| object.get_mut(*part)) else { return false; };
        cursor = next;
    }
    cursor.as_object_mut().and_then(|object| object.remove(*last)).is_some()
}

/// Apply only transformations explicitly authorized by a verified profile.
/// Rejected/unsupported fields are reported, not silently reinterpreted.
pub fn adapt_request_for_model(profile: &FrontierModelProfileV2, request: &Value) -> RequestCompatibilityV2 {
    let mut safe_request = request.clone();
    let mut removed_parameters = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for key in profile.request_constraints.deprecated_parameters.iter()
        .chain(profile.request_constraints.ignored_parameters.iter())
    {
        if delete_path(&mut safe_request, key) {
            removed_parameters.push(key.clone());
            warnings.push(format!("removed `{key}` because the verified profile marks it deprecated/ignored"));
        }
    }
    for key in &profile.request_constraints.rejected_parameters {
        if has_path(&safe_request, key) { errors.push(format!("`{key}` is rejected/incompatible with this model profile")); }
    }
    for key in &profile.request_constraints.unsupported_parameters {
        if has_path(&safe_request, key) { errors.push(format!("`{key}` is unsupported by this model profile")); }
    }
    if let Some(turns) = safe_request.get("messages").and_then(Value::as_array) {
        if let Some(role) = turns.last().and_then(|turn| turn.get("role")).and_then(Value::as_str) {
            if role.eq_ignore_ascii_case("assistant") && profile.request_constraints.unsupported_turn_patterns.iter().any(|value| value == "assistant-prefill") {
                errors.push("assistant response prefilling is unsupported by this model profile".into());
            }
        }
    }
    if let Some(turns) = safe_request.get("contents").and_then(Value::as_array) {
        if let Some(role) = turns.last().and_then(|turn| turn.get("role")).and_then(Value::as_str) {
            if role.eq_ignore_ascii_case("model") && profile.request_constraints.unsupported_turn_patterns.iter().any(|value| value == "prefilled-model-turn") {
                errors.push("prefilled final model turns are unsupported by this model profile".into());
            }
        }
    }
    for migration in &profile.request_constraints.parameter_migrations {
        if has_path(&safe_request, &migration.from) {
            warnings.push(format!("{}: {}", migration.from, migration.note));
        }
    }

    RequestCompatibilityV2 {
        provider: profile.provider.clone(), model: profile.model.clone(), preferred_api_surface: profile.preferred_api_surface.clone(),
        safe_request, removed_parameters, warnings, compatible: errors.is_empty(), errors,
        source_kind: profile.source_kind.clone(), synthetic: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> FrontierModelProfileV2 {
        FrontierModelProfileV2 {
            provider: "provider".into(), model: "model".into(), aliases: vec!["latest".into()], availability: Some(ModelAvailability::Ga),
            released_at: None, verified_at: Some("2026-08-10T00:00:00Z".into()), stale_after_days: Some(30), context_window_tokens: Some(1_000_000), max_output_tokens: Some(128_000), knowledge_cutoff: None,
            input_modalities: vec!["text".into()], output_modalities: vec!["text".into()], api_surfaces: vec!["responses".into()], preferred_api_surface: Some("responses".into()),
            reasoning: ReasoningCapabilityV2::default(), tools: ToolCapabilityV2::default(),
            request_constraints: RequestConstraintsV2 { deprecated_parameters: vec!["temperature".into()], rejected_parameters: vec!["stop".into()], ..Default::default() },
            tokenizer_notes: vec![], declared_sources: vec![], source_kind: Some("system-verified-primary-source".into()),
        }
    }

    #[test]
    fn alias_matching_is_case_insensitive() {
        assert!(profile().matches("PROVIDER", "LATEST"));
    }

    #[test]
    fn adapter_removes_only_documented_safe_fields_and_reports_rejections() {
        let request = serde_json::json!({"temperature": 0.7, "stop": ["END"], "input": "x"});
        let result = adapt_request_for_model(&profile(), &request);
        assert!(result.safe_request.get("temperature").is_none());
        assert!(result.safe_request.get("input").is_some());
        assert!(!result.compatible);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn calendar_does_not_promote_deprecated_provider_state() {
        let record = FrontierModelLifecycleRecord {
            provider: "anthropic".into(),
            model: "example".into(),
            provider_state: ProviderLifecycleState::Deprecated,
            deprecated_at: Some("2026-06-05T00:00:00Z".into()),
            retirement_at: Some("2026-08-05T00:00:00Z".into()),
            retirement_date_kind: Some(LifecycleDateKind::Tentative),
            recommended_replacement: Some("replacement".into()),
            verified_at: "2026-08-10T00:00:00Z".into(),
            source: LifecycleEvidenceSource { kind: "official-doc".into(), reference: "https://example.invalid".into(), observed_at: "2026-08-10T00:00:00Z".into() },
            note: "Provider still documents Deprecated.".into(),
            synthetic: false,
        };
        assert_eq!(record.provider_state, ProviderLifecycleState::Deprecated);
        assert!(record.migration_required());
        assert!(!record.execution_eligible());
        assert_eq!(record.retirement_date_has_passed(chrono::Utc::now()), Some(true));
    }
}
