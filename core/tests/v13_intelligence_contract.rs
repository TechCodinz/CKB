use ckb_core::{
    architecture_query_manifest,
    parse_architecture_query,
    ArchitectureQueryOperation,
    ArchitectureTask,
    ContextBudget,
    ContextCompiler,
    EvidenceClass,
    EvidenceRecord,
    MemoryEdge,
    MemoryNode,
    MemoryQueryResult,
    ModelCapabilityProfile,
    OtlpReceiver,
    PromotionDecision,
    PromotionGate,
    EvolutionProposal,
};
use ckb_core::types::NodeId;

#[test]
fn public_aql_is_deterministic_and_model_neutral() {
    let op = parse_architecture_query("PATH src/a.ts::start -> src/b.ts::work DEPTH 5").unwrap();
    assert_eq!(op, ArchitectureQueryOperation::Path {
        source: "src/a.ts::start".into(),
        target: "src/b.ts::work".into(),
        depth: 5,
    });
    let manifest = architecture_query_manifest();
    assert_eq!(manifest.version, "ckb-aql-v1");
    assert_eq!(manifest.natural_language_fallback, "MEMORY");
}

#[test]
fn ambiguous_otlp_identity_is_not_a_source_symbol() {
    let payload = r#"[
      {"name":"work","startTimeUnixNano":"0","endTimeUnixNano":"1000000","attributes":[
        {"key":"service.name","value":{"stringValue":"billing"}},
        {"key":"code.function.name","value":{"stringValue":"work"}}
      ],"status":{"code":1}}
    ]"#;
    let metrics = OtlpReceiver::ingest_spans(payload).unwrap();
    let id = metrics.keys().next().unwrap();
    assert!(OtlpReceiver::is_unresolved_runtime_identity(id));
    assert_ne!(id, &NodeId("work".into()));
    assert_ne!(id, &NodeId("billing::work".into()));
}

#[test]
fn compiled_context_never_promotes_predicted_provenance_to_runtime() {
    let memory = MemoryQueryResult {
        query: "change checkout".into(),
        root_ids: vec!["src/checkout.ts::checkout".into()],
        nodes: vec![MemoryNode {
            id: "src/checkout.ts::checkout".into(),
            name: "checkout".into(),
            kind: "function".into(),
            path: "src/checkout.ts".into(),
            line: 10,
            column: 1,
            fan_in: 3,
            fan_out: 2,
            activity_priority: 0.5,
            runtime: None,
            evidence: vec![EvidenceRecord {
                class: EvidenceClass::Predicted,
                source: "blast-radius".into(),
                reference: "prediction-1".into(),
                confidence: Some(0.9),
                observed_at: None,
                synthetic: false,
            }],
        }],
        edges: vec![MemoryEdge {
            source: "src/checkout.ts::checkout".into(),
            target: "src/payments.ts::charge".into(),
            kind: "call".into(),
            evidence: vec![],
        }],
        retrieval: Default::default(),
        version: "ckb-memory-v1".into(),
        synthetic: false,
    };
    let profile = ModelCapabilityProfile {
        provider: Some("future-provider".into()),
        model: Some("future-model".into()),
        context_window_tokens: Some(2_000_000),
        supports_structured_output: true,
        supports_tool_use: true,
        supports_parallel_tools: true,
        supports_images: true,
        supports_code_execution: true,
    };
    let compiled = ContextCompiler::compile(
        &memory,
        ArchitectureTask::Change,
        profile,
        ContextBudget { max_chars: 20_000, max_nodes: 20, max_edges: 20 },
    );
    assert_eq!(compiled.runtime_evidence_records, 0);
    assert_eq!(compiled.predicted_evidence_records, 1);
    assert!(compiled.evidence_ledger.iter().all(|record| record.class != EvidenceClass::Runtime));
}

#[test]
fn production_self_evolution_requires_explicit_promotion_and_validation() {
    let proposal = EvolutionProposal {
        id: "proposal-1".into(),
        summary: "change architecture engine".into(),
        production_affecting: true,
        validation: Default::default(),
        explicit_promotion: false,
        synthetic: false,
    };
    let decision = PromotionGate::evaluate(&proposal);
    assert!(matches!(decision, PromotionDecision::Blocked { .. }));
}
