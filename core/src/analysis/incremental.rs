//! Exact incremental repository learning over persisted parsed evidence.
//!
//! The expensive/source-sensitive step is parsing. CKB stores normalized
//! `FileAnalysis` evidence (not source text), reparses only verified changed
//! files, then deterministically rebuilds cross-file resolution from the full
//! set of parsed evidence. This avoids stale import/call edges without forcing
//! every unchanged source file through Tree-sitter again.
//!
//! If the parsed-evidence state is absent or inconsistent, callers must fall
//! back to a full verified scan. This module never guesses a delta.

use crate::graph::DependencyGraph;
use crate::parser::FileAnalysis;
use crate::types::{NodeId, RuntimeMetrics};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const INCREMENTAL_STATE_VERSION: &str = "ckb-parsed-evidence-state-v1";
pub const INCREMENTAL_DELTA_VERSION: &str = "ckb-incremental-learning-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileDeltaKind {
    Add,
    Modify,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedFileDelta {
    pub path: String,
    pub kind: FileDeltaKind,
    /// Required for add/modify; must be parsed from the verified new source.
    /// Delete deliberately carries no replacement analysis.
    pub analysis: Option<FileAnalysis>,
    /// Optional caller-provided immutable source identity (Git blob SHA,
    /// content digest, etc.). CKB records it but never fabricates it.
    pub source_digest: Option<String>,
    /// Provenance of the change event: e.g. git-commit, guarded-change,
    /// ide-save, repository-webhook. This is descriptive, not trust elevation.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFileEvidence {
    pub analysis: FileAnalysis,
    pub source_digest: Option<String>,
    pub last_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryAnalysisState {
    pub version: String,
    /// Snapshot this parsed-evidence set is known to describe. Standalone CLI
    /// states may leave this unset; server-side incremental promotion requires
    /// a matching anchor and falls back to a full scan otherwise.
    #[serde(default)]
    pub snapshot_id: Option<String>,
    pub files: BTreeMap<String, ParsedFileEvidence>,
    pub synthetic: bool,
}

impl Default for RepositoryAnalysisState {
    fn default() -> Self {
        Self {
            version: INCREMENTAL_STATE_VERSION.into(),
            snapshot_id: None,
            files: BTreeMap::new(),
            synthetic: false,
        }
    }
}

impl RepositoryAnalysisState {
    pub fn from_completed_scan(analyses: Vec<FileAnalysis>) -> Result<Self> {
        let mut state = Self::default();
        for analysis in analyses {
            let path = normalize_identity_path(&analysis.path);
            if path.is_empty() { return Err(anyhow!("completed scan contains an empty analysis path")); }
            if state.files.contains_key(&path) {
                return Err(anyhow!("completed scan contains duplicate parsed evidence for {path}"));
            }
            state.files.insert(path, ParsedFileEvidence {
                analysis,
                source_digest: None,
                last_source: "completed-scan".into(),
            });
        }
        Ok(state)
    }

    pub fn anchor_snapshot(&mut self, snapshot_id: impl Into<String>) {
        self.snapshot_id = Some(snapshot_id.into());
    }

    pub fn require_snapshot(&self, expected: &str) -> Result<()> {
        match self.snapshot_id.as_deref() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(anyhow!(
                "parsed evidence describes snapshot {actual}, current architecture snapshot is {expected}; full scan required"
            )),
            None => Err(anyhow!("parsed evidence has no snapshot anchor; full scan required")),
        }
    }

    pub fn file_count(&self) -> usize { self.files.len() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalLearningReport {
    pub version: String,
    pub changed_files: Vec<String>,
    pub added_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub deleted_files: Vec<String>,
    /// Number of source files that had to be parsed for this delta. Unchanged
    /// files reuse persisted normalized parsed evidence.
    pub reparsed_files: usize,
    /// Cross-file relationships are rebuilt from all stored parsed evidence so
    /// unchanged callers/importers can reconnect to changed definitions.
    pub relationship_evidence_files: usize,
    pub before_nodes: usize,
    pub before_edges: usize,
    pub after_nodes: usize,
    pub after_edges: usize,
    /// Runtime observations retained only for source files untouched by this
    /// delta and exact node IDs that still exist in the rebuilt graph.
    pub runtime_observations_retained: usize,
    /// Pre-change runtime observations intentionally detached because their
    /// source file changed/disappeared or their exact node identity vanished.
    pub runtime_observations_dropped: usize,
    pub exact_relationship_rebuild: bool,
    pub full_source_rescan_required: bool,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub struct IncrementalArchitectureEngine;

impl IncrementalArchitectureEngine {
    /// Deterministically construct a graph from normalized parsed evidence.
    pub fn graph_from_state(state: &RepositoryAnalysisState) -> Result<DependencyGraph> {
        if state.version != INCREMENTAL_STATE_VERSION {
            return Err(anyhow!("unsupported parsed-evidence state version: {}", state.version));
        }
        let mut graph = DependencyGraph::new();
        for evidence in state.files.values() {
            graph.add_file(&evidence.analysis)?;
        }
        graph.build_call_graph()?;
        graph.build_type_graph()?;
        Ok(graph)
    }

    /// Apply verified parsed deltas and rebuild relationship resolution without
    /// reparsing unchanged source. Pre-change runtime observations are retained
    /// only for exact node IDs in files that were not changed by this delta.
    /// A modified source file must produce new telemetry before CKB labels its
    /// post-change runtime as observed.
    pub fn apply_verified_delta(
        current_graph: &DependencyGraph,
        state: &mut RepositoryAnalysisState,
        deltas: Vec<VerifiedFileDelta>,
    ) -> Result<(DependencyGraph, IncrementalLearningReport)> {
        if state.version != INCREMENTAL_STATE_VERSION {
            return Err(anyhow!("parsed-evidence state is incompatible; full scan required"));
        }
        if deltas.is_empty() {
            return Err(anyhow!("at least one verified file delta is required"));
        }

        let before_nodes = current_graph.node_count();
        let before_edges = current_graph.edge_count();
        let previous_runtime = runtime_snapshot(current_graph);
        let mut seen = BTreeSet::new();
        let mut changed_files = Vec::new();
        let mut added_files = Vec::new();
        let mut modified_files = Vec::new();
        let mut deleted_files = Vec::new();
        let mut reparsed_files = 0usize;

        for delta in &deltas {
            let path = normalize_identity_path(&delta.path);
            if path.is_empty() { return Err(anyhow!("incremental delta contains an empty path")); }
            if !seen.insert(path.clone()) { return Err(anyhow!("duplicate delta for {path}")); }
            match delta.kind {
                FileDeltaKind::Add | FileDeltaKind::Modify => {
                    let analysis = delta.analysis.as_ref().ok_or_else(|| anyhow!("{path} requires parsed evidence for add/modify"))?;
                    if normalize_identity_path(&analysis.path) != path {
                        return Err(anyhow!("delta path {path} does not match parsed analysis path {}", analysis.path));
                    }
                }
                FileDeltaKind::Delete => {
                    if delta.analysis.is_some() {
                        return Err(anyhow!("delete delta for {path} must not contain replacement parsed evidence"));
                    }
                }
            }
        }

        let changed_path_set: HashSet<String> = seen.iter().cloned().collect();
        let mut next_state = state.clone();
        for delta in deltas {
            let path = normalize_identity_path(&delta.path);
            changed_files.push(path.clone());
            match delta.kind {
                FileDeltaKind::Add => {
                    if next_state.files.contains_key(&path) {
                        return Err(anyhow!("cannot add {path}: parsed evidence already exists; use modify"));
                    }
                    let analysis = delta.analysis.expect("validated add analysis");
                    next_state.files.insert(path.clone(), ParsedFileEvidence {
                        analysis, source_digest: delta.source_digest, last_source: delta.source,
                    });
                    added_files.push(path);
                    reparsed_files += 1;
                }
                FileDeltaKind::Modify => {
                    if !next_state.files.contains_key(&path) {
                        return Err(anyhow!("cannot modify {path}: no prior parsed evidence exists; use add or full scan"));
                    }
                    let analysis = delta.analysis.expect("validated modify analysis");
                    next_state.files.insert(path.clone(), ParsedFileEvidence {
                        analysis, source_digest: delta.source_digest, last_source: delta.source,
                    });
                    modified_files.push(path);
                    reparsed_files += 1;
                }
                FileDeltaKind::Delete => {
                    if next_state.files.remove(&path).is_none() {
                        return Err(anyhow!("cannot delete {path}: no prior parsed evidence exists"));
                    }
                    deleted_files.push(path);
                }
            }
        }

        let mut next_graph = Self::graph_from_state(&next_state)?;
        let surviving_ids: HashSet<NodeId> = next_graph.get_all_nodes().into_iter().map(|node| node.id).collect();
        let mut runtime_observations_retained = 0usize;
        let mut runtime_observations_dropped = 0usize;
        for (id, node_path, metrics) in previous_runtime {
            let source_unchanged = !changed_path_set.contains(&normalize_identity_path(&node_path));
            if source_unchanged && surviving_ids.contains(&id) {
                next_graph.record_runtime_metrics(id, metrics);
                runtime_observations_retained += 1;
            } else {
                runtime_observations_dropped += 1;
            }
        }

        *state = next_state;
        changed_files.sort();
        added_files.sort();
        modified_files.sort();
        deleted_files.sort();
        let report = IncrementalLearningReport {
            version: INCREMENTAL_DELTA_VERSION.into(),
            changed_files,
            added_files,
            modified_files,
            deleted_files,
            reparsed_files,
            relationship_evidence_files: state.files.len(),
            before_nodes,
            before_edges,
            after_nodes: next_graph.node_count(),
            after_edges: next_graph.edge_count(),
            runtime_observations_retained,
            runtime_observations_dropped,
            exact_relationship_rebuild: true,
            full_source_rescan_required: false,
            evidence_policy: "unchanged-source-reuses-parsed-evidence; relationships-rebuilt-exactly; changed-source-runtime-detached-until-reobserved".into(),
            synthetic: false,
        };
        Ok((next_graph, report))
    }
}

fn runtime_snapshot(graph: &DependencyGraph) -> Vec<(NodeId, String, RuntimeMetrics)> {
    graph.get_all_nodes().into_iter()
        .filter_map(|node| {
            graph.get_runtime_metrics(&node.id).cloned().map(|runtime| {
                (node.id, node.path.to_string_lossy().to_string(), runtime)
            })
        })
        .collect()
}

fn normalize_identity_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut parts: Vec<String> = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => { parts.pop(); }
            value => parts.push(value.to_string()),
        }
    }
    let prefix = if replaced.starts_with('/') { "/" } else { "" };
    format!("{}{}", prefix, parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, Import, ImportKind, Node, NodeKind};
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

    fn analysis(path: &str, function: Option<&str>, import: Option<&str>, call: Option<(&str, &str)>) -> FileAnalysis {
        let mut nodes = vec![node(path, path, NodeKind::File)];
        if let Some(function) = function { nodes.push(node(path, function, NodeKind::Function)); }
        FileAnalysis {
            path: path.into(), nodes,
            imports: import.map(|source| vec![Import { source: source.into(), symbols: vec![], kind: ImportKind::Named }]).unwrap_or_default(),
            exports: vec![],
            calls: call.map(|(caller, callee)| vec![FunctionCall { caller_name: caller.into(), callee_name: callee.into(), line: 2, column: 1 }]).unwrap_or_default(),
            type_relations: vec![],
        }
    }

    #[test]
    fn snapshot_anchor_detects_incompatible_incremental_state() {
        let mut state = RepositoryAnalysisState::from_completed_scan(vec![analysis("src/a.ts", Some("a"), None, None)]).unwrap();
        state.anchor_snapshot("snap-a");
        assert!(state.require_snapshot("snap-a").is_ok());
        assert!(state.require_snapshot("snap-b").is_err());
    }

    #[test]
    fn unchanged_callers_reconnect_to_modified_definition_without_reparse() {
        let a = analysis("src/a.ts", Some("start"), None, Some(("start", "work")));
        let b = analysis("src/b.ts", Some("work"), None, None);
        let mut state = RepositoryAnalysisState::from_completed_scan(vec![a, b.clone()]).unwrap();
        let graph = IncrementalArchitectureEngine::graph_from_state(&state).unwrap();
        assert!(graph.get_callees(&NodeId("src/a.ts::start".into())).contains(&NodeId("src/b.ts::work".into())));

        let mut changed_b = b;
        changed_b.nodes.iter_mut().find(|node| node.name == "work").unwrap().line = 99;
        let (next, report) = IncrementalArchitectureEngine::apply_verified_delta(&graph, &mut state, vec![VerifiedFileDelta {
            path: "src/b.ts".into(), kind: FileDeltaKind::Modify, analysis: Some(changed_b), source_digest: Some("git-blob-2".into()), source: "git-commit".into(),
        }]).unwrap();
        assert_eq!(report.reparsed_files, 1);
        assert!(report.exact_relationship_rebuild);
        assert!(next.get_callees(&NodeId("src/a.ts::start".into())).contains(&NodeId("src/b.ts::work".into())));
    }

    #[test]
    fn modified_symbol_requires_new_runtime_observation_even_if_id_survives() {
        let a = analysis("src/a.ts", Some("work"), None, None);
        let mut state = RepositoryAnalysisState::from_completed_scan(vec![a.clone()]).unwrap();
        let mut graph = IncrementalArchitectureEngine::graph_from_state(&state).unwrap();
        graph.record_runtime_metrics(NodeId("src/a.ts::work".into()), RuntimeMetrics { execution_count: 5, avg_latency_ms: 10.0, error_rate: 0.0, is_hotpath: false });
        let mut replacement = a;
        replacement.nodes.iter_mut().find(|node| node.name == "work").unwrap().line = 8;
        let (next, report) = IncrementalArchitectureEngine::apply_verified_delta(&graph, &mut state, vec![VerifiedFileDelta {
            path: "src/a.ts".into(), kind: FileDeltaKind::Modify, analysis: Some(replacement), source_digest: None, source: "guarded-change".into(),
        }]).unwrap();
        assert!(next.get_runtime_metrics(&NodeId("src/a.ts::work".into())).is_none());
        assert_eq!(report.runtime_observations_dropped, 1);
    }

    #[test]
    fn deleted_symbol_does_not_inherit_runtime_by_name() {
        let a = analysis("src/a.ts", Some("work"), None, None);
        let mut state = RepositoryAnalysisState::from_completed_scan(vec![a]).unwrap();
        let mut graph = IncrementalArchitectureEngine::graph_from_state(&state).unwrap();
        graph.record_runtime_metrics(NodeId("src/a.ts::work".into()), RuntimeMetrics { execution_count: 5, avg_latency_ms: 10.0, error_rate: 0.0, is_hotpath: false });
        let replacement = analysis("src/a.ts", Some("work2"), None, None);
        let (next, report) = IncrementalArchitectureEngine::apply_verified_delta(&graph, &mut state, vec![VerifiedFileDelta {
            path: "src/a.ts".into(), kind: FileDeltaKind::Modify, analysis: Some(replacement), source_digest: None, source: "guarded-change".into(),
        }]).unwrap();
        assert!(next.get_runtime_metrics(&NodeId("src/a.ts::work2".into())).is_none());
        assert_eq!(report.runtime_observations_dropped, 1);
    }

    #[test]
    fn invalid_batch_does_not_partially_mutate_state() {
        let a = analysis("src/a.ts", Some("a"), None, None);
        let mut state = RepositoryAnalysisState::from_completed_scan(vec![a]).unwrap();
        let graph = IncrementalArchitectureEngine::graph_from_state(&state).unwrap();
        let before = state.file_count();
        let result = IncrementalArchitectureEngine::apply_verified_delta(&graph, &mut state, vec![
            VerifiedFileDelta { path: "src/b.ts".into(), kind: FileDeltaKind::Add, analysis: Some(analysis("src/b.ts", Some("b"), None, None)), source_digest: None, source: "test".into() },
            VerifiedFileDelta { path: "src/missing.ts".into(), kind: FileDeltaKind::Delete, analysis: None, source_digest: None, source: "test".into() },
        ]);
        assert!(result.is_err());
        assert_eq!(state.file_count(), before);
        assert!(!state.files.contains_key("src/b.ts"));
    }
}
