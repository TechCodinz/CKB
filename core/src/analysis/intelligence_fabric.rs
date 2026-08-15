//! CKB V13 model-agnostic Architecture Intelligence Fabric.
//!
//! This layer is intentionally independent of any model vendor or product
//! generation. Frontier models can change without changing CKB's truth model:
//! CKB owns evidence, architecture memory, context compilation, evaluation and
//! promotion gates; models consume those contracts.

use super::{ArchitectureMemorySlice, MemoryEvidence};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FABRIC_VERSION: &str = "ckb-intelligence-fabric-v13";
pub const CONTEXT_VERSION: &str = "ckb-context-v1";
pub const EVIDENCE_POLICY: &str = "static-runtime-predicted-separated";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Static,
    Runtime,
    Predicted,
    History,
    Human,
    Validation,
}

impl EvidenceClass {
    pub fn from_kind(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "runtime" | "observed-runtime" | "telemetry" => Self::Runtime,
            "predicted" | "prediction" | "simulation" | "hypothesis" => Self::Predicted,
            "history" | "git" | "snapshot" => Self::History,
            "human" | "decision" | "intent" => Self::Human,
            "validation" | "test" | "compiler" | "contract" => Self::Validation,
            _ => Self::Static,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRecord {
    pub id: String,
    pub class: EvidenceClass,
    pub source: String,
    pub reference: String,
    pub confidence: f32,
    pub observed_at: Option<String>,
    pub synthetic: bool,
}

impl EvidenceRecord {
    pub fn from_memory(owner: &str, index: usize, evidence: &MemoryEvidence) -> Self {
        let class = EvidenceClass::from_kind(&evidence.kind);
        // Confidence is provenance strength, not probability that a prediction
        // is true. Predicted claims remain Predicted regardless of confidence.
        let confidence = match class {
            EvidenceClass::Runtime | EvidenceClass::Validation => 1.0,
            EvidenceClass::Static | EvidenceClass::History => 0.98,
            EvidenceClass::Human => 0.95,
            EvidenceClass::Predicted => 0.50,
        };
        Self {
            id: format!("{}:evidence:{}", owner, index),
            class,
            source: evidence.source.clone(),
            reference: evidence.reference.clone(),
            confidence,
            observed_at: None,
            synthetic: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstitutionRule {
    pub id: String,
    pub title: String,
    pub requirement: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureConstitution {
    pub version: String,
    pub evidence_policy: String,
    pub rules: Vec<ConstitutionRule>,
}

impl Default for ArchitectureConstitution {
    fn default() -> Self {
        Self {
            version: "ckb-architecture-constitution-v1".into(),
            evidence_policy: EVIDENCE_POLICY.into(),
            rules: vec![
                ConstitutionRule { id: "truth.runtime".into(), title: "Runtime requires observation".into(), requirement: "Never represent static reachability or predicted execution as observed runtime. Runtime claims require telemetry evidence.".into(), blocking: true },
                ConstitutionRule { id: "truth.prediction".into(), title: "Prediction remains a hypothesis".into(), requirement: "Simulations, forecasts and impact estimates remain PREDICTED until independently observed or validated.".into(), blocking: true },
                ConstitutionRule { id: "truth.provenance".into(), title: "Claims require provenance".into(), requirement: "Architecture claims exposed to models must retain their evidence source and stable reference where available.".into(), blocking: true },
                ConstitutionRule { id: "identity.source".into(), title: "Prefer stable source identity".into(), requirement: "Use repository-relative paths, stable symbol IDs and source spans. Do not substitute temporary filesystem paths for repository identity.".into(), blocking: true },
                ConstitutionRule { id: "change.no-silent-mutation".into(), title: "No silent source mutation".into(), requirement: "A reasoning or proposal operation must not silently mutate, merge, push or deploy source code.".into(), blocking: true },
                ConstitutionRule { id: "change.validate-before-promote".into(), title: "Validate before promotion".into(), requirement: "Code-changing proposals require explicit validation evidence before promotion; critical changes require compiler/test/contract evidence appropriate to the repository.".into(), blocking: true },
                ConstitutionRule { id: "security.context-minimization".into(), title: "Minimize model context".into(), requirement: "Compile bounded task-specific context. Do not expose secrets or unrelated repository contents merely because a model has a large context window.".into(), blocking: true },
                ConstitutionRule { id: "evolution.guarded".into(), title: "Self-evolution is guarded".into(), requirement: "CKB may learn from observed outcomes and propose improvements, but production-changing self-modification must pass the same simulation, validation and explicit promotion gates as other changes.".into(), blocking: true },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchitectureTaskKind {
    Understand,
    Explain,
    Change,
    Debug,
    Review,
    Migrate,
    Optimize,
    Security,
}

impl Default for ArchitectureTaskKind {
    fn default() -> Self { Self::Understand }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilityProfile {
    /// Informational identity only. Provider/model names never alter CKB truth.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_window_tokens: Option<usize>,
    pub supports_structured_output: bool,
    pub supports_tool_use: bool,
    pub supports_parallel_tools: bool,
    pub supports_images: bool,
    pub supports_code_execution: bool,
}

impl Default for ModelCapabilityProfile {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            context_window_tokens: None,
            supports_structured_output: true,
            supports_tool_use: true,
            supports_parallel_tools: false,
            supports_images: false,
            supports_code_execution: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudget {
    pub max_chars: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self { max_chars: 48_000, max_nodes: 80, max_edges: 160 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSection {
    pub id: String,
    pub evidence_class: Option<EvidenceClass>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledModelContext {
    pub version: String,
    pub fabric_version: String,
    pub task: ArchitectureTaskKind,
    pub query: String,
    pub source_memory_version: String,
    pub source_root_ids: Vec<String>,
    pub sections: Vec<ContextSection>,
    pub evidence_ledger: Vec<EvidenceRecord>,
    pub included_nodes: usize,
    pub included_edges: usize,
    pub runtime_evidence_records: usize,
    pub predicted_evidence_records: usize,
    pub char_count: usize,
    pub truncated: bool,
    pub evidence_policy: String,
    pub constitution: ArchitectureConstitution,
    pub model_profile: ModelCapabilityProfile,
    pub synthetic: bool,
}

pub struct ContextCompiler;

impl ContextCompiler {
    fn push_bounded(sections: &mut Vec<ContextSection>, chars: &mut usize, budget: usize, section: ContextSection) -> bool {
        let size = section.content.chars().count();
        if *chars + size > budget { return false; }
        *chars += size;
        sections.push(section);
        true
    }

    fn task_guidance(task: ArchitectureTaskKind) -> &'static str {
        match task {
            ArchitectureTaskKind::Understand => "Map responsibilities and boundaries before suggesting changes.",
            ArchitectureTaskKind::Explain => "Explain from evidence and name uncertainty explicitly.",
            ArchitectureTaskKind::Change => "Identify contracts, callers/callees, tests and blast radius before proposing mutation.",
            ArchitectureTaskKind::Debug => "Prioritize observed runtime/error evidence; do not infer an executed path from static edges alone.",
            ArchitectureTaskKind::Review => "Check architecture boundaries, coupling, contracts, tests and evidence-backed regression risk.",
            ArchitectureTaskKind::Migrate => "Preserve external contracts and map old/new boundaries with explicit transition risk.",
            ArchitectureTaskKind::Optimize => "Prefer observed hotpaths/latency evidence; static centrality alone is not performance evidence.",
            ArchitectureTaskKind::Security => "Minimize exposed context and inspect trust boundaries, inputs, outputs and high-impact dependency paths.",
        }
    }

    pub fn compile(
        memory: &ArchitectureMemorySlice,
        task: ArchitectureTaskKind,
        model_profile: ModelCapabilityProfile,
        budget: ContextBudget,
    ) -> CompiledModelContext {
        let constitution = ArchitectureConstitution::default();
        let budget_chars = budget.max_chars.clamp(4_000, 500_000);
        let mut sections = Vec::new();
        let mut char_count = 0usize;
        let mut ledger = Vec::new();
        let mut truncated = memory.retrieval.truncated;

        let constitution_text = format!(
            "CKB TRUTH CONTRACT\nEvidence policy: {}\nTask: {:?}\nGuidance: {}\nRules:\n{}",
            EVIDENCE_POLICY,
            task,
            Self::task_guidance(task),
            constitution.rules.iter().map(|rule| format!("- {}: {}", rule.id, rule.requirement)).collect::<Vec<_>>().join("\n")
        );
        let _ = Self::push_bounded(&mut sections, &mut char_count, budget_chars, ContextSection {
            id: "truth-contract".into(), evidence_class: None, content: constitution_text,
        });

        let root_text = format!("Query: {}\nMemory roots: {}", memory.query, memory.root_ids.join(", "));
        let _ = Self::push_bounded(&mut sections, &mut char_count, budget_chars, ContextSection {
            id: "task-roots".into(), evidence_class: Some(EvidenceClass::Static), content: root_text,
        });

        let mut included_nodes = 0usize;
        for node in memory.nodes.iter().take(budget.max_nodes.max(1)) {
            let mut evidence_labels = Vec::new();
            for (index, evidence) in node.evidence.iter().enumerate() {
                let record = EvidenceRecord::from_memory(&node.id, index, evidence);
                evidence_labels.push(format!("{:?}:{}:{}", record.class, record.source, record.reference));
                ledger.push(record);
            }
            let runtime = node.runtime.as_ref().map(|value| format!(
                "OBSERVED_RUNTIME calls={} avg_latency_ms={:.2} error_rate={:.4} hotpath={}",
                value.execution_count, value.avg_latency_ms, value.error_rate, value.is_hotpath
            )).unwrap_or_else(|| "OBSERVED_RUNTIME none attached".into());
            let content = format!(
                "{} [{}]\nid={}\nsource={}:{}:{}\nfan_in={} fan_out={} activity={:.3}\n{}\nprovenance={}",
                node.name, node.kind, node.id, node.path, node.line, node.column,
                node.fan_in, node.fan_out, node.activity_priority, runtime, evidence_labels.join(" | ")
            );
            if !Self::push_bounded(&mut sections, &mut char_count, budget_chars, ContextSection {
                id: format!("node:{}", node.id), evidence_class: Some(EvidenceClass::Static), content,
            }) {
                truncated = true;
                break;
            }
            included_nodes += 1;
        }

        let included_ids: BTreeSet<&str> = memory.nodes.iter().take(included_nodes).map(|node| node.id.as_str()).collect();
        let mut included_edges = 0usize;
        for (edge_index, edge) in memory.edges.iter()
            .filter(|edge| included_ids.contains(edge.source.as_str()) && included_ids.contains(edge.target.as_str()))
            .take(budget.max_edges.max(1))
            .enumerate()
        {
            for (index, evidence) in edge.evidence.iter().enumerate() {
                ledger.push(EvidenceRecord::from_memory(&format!("edge:{}", edge_index), index, evidence));
            }
            let content = format!("{} --{}--> {}", edge.source, edge.kind, edge.target);
            if !Self::push_bounded(&mut sections, &mut char_count, budget_chars, ContextSection {
                id: format!("edge:{}", edge_index), evidence_class: Some(EvidenceClass::Static), content,
            }) {
                truncated = true;
                break;
            }
            included_edges += 1;
        }

        // Deterministic dedupe keeps the ledger compact without losing class,
        // source or reference provenance.
        let mut seen = BTreeSet::new();
        ledger.retain(|record| seen.insert(format!("{:?}|{}|{}", record.class, record.source, record.reference)));
        let runtime_evidence_records = ledger.iter().filter(|item| item.class == EvidenceClass::Runtime).count();
        let predicted_evidence_records = ledger.iter().filter(|item| item.class == EvidenceClass::Predicted).count();

        CompiledModelContext {
            version: CONTEXT_VERSION.into(),
            fabric_version: FABRIC_VERSION.into(),
            task,
            query: memory.query.clone(),
            source_memory_version: memory.version.clone(),
            source_root_ids: memory.root_ids.clone(),
            sections,
            evidence_ledger: ledger,
            included_nodes,
            included_edges,
            runtime_evidence_records,
            predicted_evidence_records,
            char_count,
            truncated,
            evidence_policy: EVIDENCE_POLICY.into(),
            constitution,
            model_profile,
            synthetic: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationObservation {
    pub evaluation_id: String,
    pub project_id: String,
    pub task: ArchitectureTaskKind,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub proposal_id: Option<String>,
    pub observed_at: String,
    pub compile_passed: Option<bool>,
    pub tests_passed: Option<bool>,
    pub contracts_passed: Option<bool>,
    pub security_checks_passed: Option<bool>,
    pub runtime_regression_observed: Option<bool>,
    pub rollback_required: Option<bool>,
    pub validation_references: Vec<String>,
    pub notes: Option<String>,
    pub synthetic: bool,
}

impl EvaluationObservation {
    pub fn observed_score(&self) -> Option<f32> {
        let mut values = Vec::new();
        for value in [self.compile_passed, self.tests_passed, self.contracts_passed, self.security_checks_passed] {
            if let Some(value) = value { values.push(if value { 1.0 } else { 0.0 }); }
        }
        if let Some(regressed) = self.runtime_regression_observed { values.push(if regressed { 0.0 } else { 1.0 }); }
        if let Some(rollback) = self.rollback_required { values.push(if rollback { 0.0 } else { 1.0 }); }
        if values.is_empty() { None } else { Some(values.iter().sum::<f32>() / values.len() as f32) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelScorecard {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub observations: usize,
    pub by_task: BTreeMap<ArchitectureTaskKind, usize>,
    pub mean_observed_score: Option<f32>,
    pub rollback_rate: Option<f32>,
    pub synthetic: bool,
}

impl ModelScorecard {
    pub fn from_observations(items: &[EvaluationObservation], provider: Option<&str>, model: Option<&str>) -> Self {
        let filtered = items.iter().filter(|item| {
            provider.map(|value| item.provider.as_deref() == Some(value)).unwrap_or(true)
                && model.map(|value| item.model.as_deref() == Some(value)).unwrap_or(true)
        }).collect::<Vec<_>>();
        let mut by_task = BTreeMap::new();
        let mut scores = Vec::new();
        let mut rollbacks = Vec::new();
        for item in &filtered {
            *by_task.entry(item.task).or_insert(0) += 1;
            if let Some(score) = item.observed_score() { scores.push(score); }
            if let Some(value) = item.rollback_required { rollbacks.push(value); }
        }
        let mean_observed_score = if scores.is_empty() { None } else { Some(scores.iter().sum::<f32>() / scores.len() as f32) };
        let rollback_rate = if rollbacks.is_empty() { None } else { Some(rollbacks.iter().filter(|value| **value).count() as f32 / rollbacks.len() as f32) };
        Self {
            provider: provider.map(str::to_string), model: model.map(str::to_string),
            observations: filtered.len(), by_task, mean_observed_score, rollback_rate, synthetic: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionProposal {
    pub id: String,
    pub summary: String,
    pub changes_production_behavior: bool,
    pub evidence_refs: Vec<String>,
    pub compile_passed: Option<bool>,
    pub tests_passed: Option<bool>,
    pub contracts_passed: Option<bool>,
    pub security_checks_passed: Option<bool>,
    pub explicit_promotion_approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionDecision {
    pub promotable: bool,
    pub blockers: Vec<String>,
    pub autonomous_promotion_allowed: bool,
}

pub struct PromotionGate;

impl PromotionGate {
    pub fn evaluate(proposal: &EvolutionProposal) -> PromotionDecision {
        let mut blockers = Vec::new();
        if proposal.evidence_refs.is_empty() { blockers.push("evidence provenance is required".into()); }
        if proposal.changes_production_behavior {
            if proposal.compile_passed != Some(true) { blockers.push("compiler/build validation has not passed".into()); }
            if proposal.tests_passed != Some(true) { blockers.push("test validation has not passed".into()); }
            if proposal.contracts_passed != Some(true) { blockers.push("contract validation has not passed".into()); }
            if proposal.security_checks_passed != Some(true) { blockers.push("security validation has not passed".into()); }
            if !proposal.explicit_promotion_approved { blockers.push("explicit promotion approval is required".into()); }
        }
        PromotionDecision {
            promotable: blockers.is_empty(), blockers,
            // V13 learns/proposes automatically but does not self-deploy source.
            autonomous_promotion_allowed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceFabricManifest {
    pub version: String,
    pub context_version: String,
    pub evidence_policy: String,
    pub model_agnostic: bool,
    pub capabilities: Vec<String>,
    pub constitution: ArchitectureConstitution,
}

pub fn intelligence_fabric_manifest() -> IntelligenceFabricManifest {
    IntelligenceFabricManifest {
        version: FABRIC_VERSION.into(),
        context_version: CONTEXT_VERSION.into(),
        evidence_policy: EVIDENCE_POLICY.into(),
        model_agnostic: true,
        capabilities: vec![
            "bounded-context-compilation".into(),
            "evidence-ledger".into(),
            "model-capability-profile".into(),
            "observed-outcome-evaluation".into(),
            "guarded-evolution-promotion".into(),
            "architecture-constitution".into(),
        ],
        constitution: ArchitectureConstitution::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{ArchitectureMemorySlice, MemoryRetrievalStats};

    fn memory_with_evidence(kind: &str) -> ArchitectureMemorySlice {
        ArchitectureMemorySlice {
            version: "test-memory".into(), query: "checkout".into(), depth: 1,
            nodes: vec![crate::analysis::MemoryNode {
                id: "src/pay.ts::charge".into(), name: "charge".into(), kind: "function".into(),
                path: "src/pay.ts".into(), line: 10, column: 1, score: 10.0, fan_in: 2, fan_out: 1,
                activity_priority: 0.5, runtime: None,
                evidence: vec![MemoryEvidence { source: "test".into(), reference: "src/pay.ts:10".into(), kind: kind.into() }],
            }],
            edges: vec![], root_ids: vec!["src/pay.ts::charge".into()], context: String::new(),
            retrieval: MemoryRetrievalStats { graph_nodes: 1, graph_edges: 0, root_count: 1, retrieved_nodes: 1, retrieved_edges: 0, runtime_observed_nodes: 0, expansion_cap: 1, truncated: false },
            evidence_policy: EVIDENCE_POLICY.into(), synthetic: false,
        }
    }

    #[test]
    fn provider_identity_does_not_change_compiled_evidence() {
        let memory = memory_with_evidence("static");
        let mut a = ModelCapabilityProfile::default(); a.provider = Some("vendor-a".into()); a.model = Some("future-1".into());
        let mut b = ModelCapabilityProfile::default(); b.provider = Some("vendor-b".into()); b.model = Some("future-99".into());
        let ca = ContextCompiler::compile(&memory, ArchitectureTaskKind::Change, a, ContextBudget::default());
        let cb = ContextCompiler::compile(&memory, ArchitectureTaskKind::Change, b, ContextBudget::default());
        assert_eq!(ca.sections.iter().map(|s| &s.content).collect::<Vec<_>>(), cb.sections.iter().map(|s| &s.content).collect::<Vec<_>>());
        assert_eq!(ca.evidence_ledger.len(), cb.evidence_ledger.len());
    }

    #[test]
    fn static_evidence_never_becomes_runtime() {
        let compiled = ContextCompiler::compile(&memory_with_evidence("static"), ArchitectureTaskKind::Debug, ModelCapabilityProfile::default(), ContextBudget::default());
        assert_eq!(compiled.runtime_evidence_records, 0);
        assert!(compiled.sections.iter().any(|s| s.content.contains("OBSERVED_RUNTIME none attached")));
    }

    #[test]
    fn context_budget_is_bounded() {
        let compiled = ContextCompiler::compile(&memory_with_evidence("static"), ArchitectureTaskKind::Understand, ModelCapabilityProfile::default(), ContextBudget { max_chars: 4_000, max_nodes: 1, max_edges: 1 });
        assert!(compiled.char_count <= 4_000);
        assert!(compiled.included_nodes <= 1);
    }

    #[test]
    fn production_evolution_requires_validation_and_explicit_promotion() {
        let proposal = EvolutionProposal { id: "p1".into(), summary: "change runtime".into(), changes_production_behavior: true, evidence_refs: vec!["ast:x".into()], compile_passed: Some(true), tests_passed: None, contracts_passed: Some(true), security_checks_passed: Some(true), explicit_promotion_approved: false };
        let decision = PromotionGate::evaluate(&proposal);
        assert!(!decision.promotable);
        assert!(!decision.autonomous_promotion_allowed);
        assert!(decision.blockers.iter().any(|item| item.contains("test")));
        assert!(decision.blockers.iter().any(|item| item.contains("explicit")));
    }
}
