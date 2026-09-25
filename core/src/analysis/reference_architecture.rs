//! License-aware reference architecture policy.
//!
//! CKB may analyze external repositories for architectural evidence, but a
//! reference repository is not automatically a source-code dependency. This
//! module keeps the distinction explicit so agents can reuse ideas without
//! silently importing incompatible code or bypass-oriented behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceLicenseClass {
    Permissive,
    Copyleft,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceArchitectureProfile {
    pub source: &'static str,
    pub license: &'static str,
    pub license_class: ReferenceLicenseClass,
    pub patterns: &'static [&'static str],
    pub constraints: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceUseDecision {
    pub pattern_use_allowed: bool,
    pub source_copy_allowed: bool,
    pub requires_license_review: bool,
    pub notes: Vec<&'static str>,
}

pub fn assess_reference_use(
    profile: &ReferenceArchitectureProfile,
    target_is_copyleft_compatible: bool,
    attribution_review_completed: bool,
) -> ReferenceUseDecision {
    match profile.license_class {
        ReferenceLicenseClass::Permissive => ReferenceUseDecision {
            pattern_use_allowed: true,
            source_copy_allowed: attribution_review_completed,
            requires_license_review: !attribution_review_completed,
            notes: vec![
                "Architecture-level reuse is allowed.",
                "Copying source still requires preserving the source license and attribution obligations.",
            ],
        },
        ReferenceLicenseClass::Copyleft => ReferenceUseDecision {
            pattern_use_allowed: true,
            source_copy_allowed: target_is_copyleft_compatible && attribution_review_completed,
            requires_license_review: true,
            notes: vec![
                "Use architecture and product patterns clean-room by default.",
                "Do not copy implementation code into a proprietary target merely because the reference is public.",
            ],
        },
        ReferenceLicenseClass::Unknown => ReferenceUseDecision {
            pattern_use_allowed: true,
            source_copy_allowed: false,
            requires_license_review: true,
            notes: vec![
                "Treat unknown licensing as pattern-only evidence.",
                "Do not import source until the license and provenance are verified.",
            ],
        },
    }
}

pub fn builtin_reference_architectures() -> Vec<ReferenceArchitectureProfile> {
    vec![
        ReferenceArchitectureProfile {
            source: "yuliskov/SmartTube",
            license: "MIT",
            license_class: ReferenceLicenseClass::Permissive,
            patterns: &[
                "media-source adapters separated from presentation/player state",
                "live-chat/community isolated from playback concerns",
                "capability-driven player/navigation states with graceful fallback",
            ],
            constraints: &[
                "Media playback capability is not evidence of broadcast rights.",
                "Prefer original implementation; preserve MIT notice if source is ever copied.",
            ],
        },
        ReferenceArchitectureProfile {
            source: "Sonarr/Sonarr",
            license: "GPL-3.0",
            license_class: ReferenceLicenseClass::Copyleft,
            patterns: &[
                "monitor -> discover -> filter -> queue -> fetch -> verify -> organize",
                "deduplicate before expensive work",
                "observable queue/retry/failure state",
                "provider adapters behind stable contracts",
            ],
            constraints: &[
                "Pattern-level inspiration only for proprietary targets unless GPL compatibility is explicitly established.",
                "Do not copy GPL implementation code into a closed-source target.",
            ],
        },
        ReferenceArchitectureProfile {
            source: "Brainicism/bgutil-ytdlp-pot-provider",
            license: "GPL-3.0",
            license_class: ReferenceLicenseClass::Copyleft,
            patterns: &[
                "provider service separated from client plugin",
                "local helper service bound to loopback by default",
                "explicit cache TTL and fail-closed provider semantics",
            ],
            constraints: &[
                "Pattern-level inspiration only for proprietary targets unless GPL compatibility is explicitly established.",
                "Do not turn this reference into anti-bot, access-control bypass or unauthorized media extraction behavior.",
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyleft_reference_is_pattern_only_for_proprietary_target() {
        let profile = builtin_reference_architectures()
            .into_iter()
            .find(|item| item.source == "Sonarr/Sonarr")
            .unwrap();
        let decision = assess_reference_use(&profile, false, true);
        assert!(decision.pattern_use_allowed);
        assert!(!decision.source_copy_allowed);
        assert!(decision.requires_license_review);
    }

    #[test]
    fn permissive_source_copy_still_requires_attribution_review() {
        let profile = builtin_reference_architectures()
            .into_iter()
            .find(|item| item.source == "yuliskov/SmartTube")
            .unwrap();
        let before = assess_reference_use(&profile, false, false);
        assert!(!before.source_copy_allowed);
        assert!(before.requires_license_review);

        let after = assess_reference_use(&profile, false, true);
        assert!(after.source_copy_allowed);
        assert!(!after.requires_license_review);
    }

    #[test]
    fn unknown_license_never_allows_source_copy() {
        let profile = ReferenceArchitectureProfile {
            source: "example/reference",
            license: "unknown",
            license_class: ReferenceLicenseClass::Unknown,
            patterns: &["queue state"],
            constraints: &[],
        };
        let decision = assess_reference_use(&profile, true, true);
        assert!(decision.pattern_use_allowed);
        assert!(!decision.source_copy_allowed);
    }
}
