use ckb_core::{
    FileAnalysis, FileDeltaKind, FunctionCall, IncrementalArchitectureEngine, Node, NodeId,
    NodeKind, RepositoryAnalysisState, VerifiedFileDelta,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn node(path: &str, name: &str, kind: NodeKind) -> Node {
    Node {
        id: NodeId(format!("{}::{}", path, if kind == NodeKind::File { "file" } else { name })),
        kind,
        name: name.into(),
        path: PathBuf::from(path),
        line: 1,
        column: 1,
        exports: vec![],
        imports: vec![],
        metadata: HashMap::new(),
    }
}

fn file(path: &str, function: &str, callee: Option<&str>) -> FileAnalysis {
    FileAnalysis {
        path: path.into(),
        nodes: vec![node(path, path, NodeKind::File), node(path, function, NodeKind::Function)],
        imports: vec![],
        exports: vec![],
        calls: callee.map(|callee| vec![FunctionCall {
            caller_name: function.into(),
            callee_name: callee.into(),
            line: 2,
            column: 1,
        }]).unwrap_or_default(),
        type_relations: vec![],
    }
}

#[test]
fn representative_incremental_graph_matches_clean_rebuild_truth() {
    // Large enough to exercise deterministic cross-file resolution without making
    // the unit test depend on network access or a particular fixture repository.
    let mut baseline = Vec::new();
    for i in 0..250usize {
        let path = format!("src/module_{i}.ts");
        let function = format!("fn_{i}");
        let callee = if i + 1 < 250 { Some(format!("fn_{}", i + 1)) } else { None };
        baseline.push(file(&path, &function, callee.as_deref()));
    }

    let mut state = RepositoryAnalysisState::from_completed_scan(baseline.clone()).unwrap();
    let baseline_graph = IncrementalArchitectureEngine::graph_from_state(&state).unwrap();

    let changed_path = "src/module_125.ts";
    let replacement = file(changed_path, "fn_125", Some("fn_200"));
    let (incremental, report) = IncrementalArchitectureEngine::apply_verified_delta(
        &baseline_graph,
        &mut state,
        vec![VerifiedFileDelta {
            path: changed_path.into(),
            kind: FileDeltaKind::Modify,
            analysis: Some(replacement.clone()),
            source_digest: Some("fixture-digest-v2".into()),
            source: "pre-vps-truth-test".into(),
        }],
    ).unwrap();

    let mut clean_evidence = baseline;
    clean_evidence[125] = replacement;
    let clean_state = RepositoryAnalysisState::from_completed_scan(clean_evidence).unwrap();
    let clean = IncrementalArchitectureEngine::graph_from_state(&clean_state).unwrap();

    assert_eq!(report.reparsed_files, 1);
    assert_eq!(report.relationship_evidence_files, 250);
    assert!(report.exact_relationship_rebuild);
    assert!(!report.full_source_rescan_required);
    assert_eq!(incremental.node_count(), clean.node_count());
    assert_eq!(incremental.edge_count(), clean.edge_count());

    let changed = NodeId("src/module_125.ts::fn_125".into());
    let expected = NodeId("src/module_200.ts::fn_200".into());
    assert_eq!(incremental.get_callees(&changed), clean.get_callees(&changed));
    assert!(incremental.get_callees(&changed).contains(&expected));

    // An unchanged caller must retain the same resolved relationship in both
    // incremental and clean truth, proving global relationship reconstruction.
    let unchanged = NodeId("src/module_124.ts::fn_124".into());
    assert_eq!(incremental.get_callees(&unchanged), clean.get_callees(&unchanged));
    assert!(incremental.get_callees(&unchanged).contains(&changed));
}
