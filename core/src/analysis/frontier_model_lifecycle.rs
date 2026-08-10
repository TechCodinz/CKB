//! Primary-source model lifecycle evidence.
//!
//! Lifecycle truth is deliberately independent of model capability truth. A
//! model may be known to be deprecated/retired even when its tool/context
//! surface has not been verified by CKB. Dates never promote lifecycle state:
//! the provider's documented state remains authoritative until re-verified.

use serde::{Deserialize, Serialize};

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

    /// A date being in the past never changes provider_state. This helper only
    /// reports date relation for migration urgency/UI; callers must not use it
    /// to infer Retired/Shutdown without fresh provider evidence.
    pub fn retirement_date_has_passed(&self, now: chrono::DateTime<chrono::Utc>) -> Option<bool> {
        let value = self.retirement_at.as_ref()?;
        let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?.with_timezone(&chrono::Utc);
        Some(now >= parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_does_not_promote_deprecated_state() {
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
